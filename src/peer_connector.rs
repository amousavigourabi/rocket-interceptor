use bytes::{Buf, BytesMut};
use log::{debug, error};
use openssl::ssl::{Ssl, SslContext, SslMethod};
use rand::prelude::*;
use std::net::{IpAddr, SocketAddr};
use std::pin::Pin;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio_openssl::SslStream;

#[derive(Clone)]
pub struct PeerConnector {
    pub ip_addr: String,
}

impl PeerConnector {
    /// Connect 2 peers using their id's and public keys.
    /// Established SSL streams between the peers.
    /// Returns 2 threads which handle the messages sent over the streams.
    pub async fn connect_peers(
        self,
        peer1_port: u16,
        peer2_port: u16,
        pub_key1: &str,
        pub_key2: &str,
    ) -> (JoinHandle<()>, JoinHandle<()>) {
        let ssl_stream_1 =
            Self::create_ssl_stream(self.ip_addr.as_str(), peer1_port, pub_key2).await;
        let ssl_stream_2 =
            Self::create_ssl_stream(self.ip_addr.as_str(), peer2_port, pub_key1).await;
        Self::handle_peer_connections(ssl_stream_1, ssl_stream_2, peer1_port, peer2_port).await
    }

    /// Create an SSL stream from a peer to another peer.
    /// Uses the current peer's ip+port and the other peer's public key.
    /// This ssl stream makes sure a peer sends messages to the interceptor first,
    /// instead of sending it straight to the other peer.
    async fn create_ssl_stream(ip: &str, port: u16, pub_key_peer_to: &str) -> SslStream<TcpStream> {
        let socket_address = SocketAddr::new(IpAddr::from_str(ip).unwrap(), port);
        let tcp_stream = match TcpStream::connect(socket_address).await {
            Ok(tcp_stream) => tcp_stream,
            Err(e) => panic!("{}", e),
        };

        tcp_stream.set_nodelay(true).expect("Set nodelay failed");
        let ssl_ctx = SslContext::builder(SslMethod::tls()).unwrap().build();
        let ssl = Ssl::new(&ssl_ctx).unwrap();
        let mut ssl_stream = SslStream::<TcpStream>::new(ssl, tcp_stream).unwrap();
        SslStream::connect(Pin::new(&mut ssl_stream))
            .await
            .expect("SSL connection failed");

        let content = Self::format_upgrade_request_content(pub_key_peer_to);
        ssl_stream
            .write_all(content.as_bytes())
            .await
            .expect("Could not send XRPL handshake request");

        let mut buf = BytesMut::new();
        let mut vec = vec![0; 4096];
        let size = ssl_stream
            .read(&mut vec)
            .await
            .expect("Unable to read handshake response");
        vec.resize(size, 0);
        buf.extend_from_slice(&vec);

        if size == 0 {
            error!("Current buffer: {}", String::from_utf8_lossy(&buf).trim());
            panic!("Socket closed");
        }

        if let Some(n) = buf.windows(4).position(|x| x == b"\r\n\r\n") {
            let mut headers = [httparse::EMPTY_HEADER; 32];
            let mut resp = httparse::Response::new(&mut headers);
            let status = resp.parse(&buf[0..n + 4]).expect("Response parse failed");
            if status.is_partial() {
                panic!("Invalid headers");
            }
            let response_code = resp.code.unwrap();

            debug!(
                "Peer Handshake Response: version {}, status {}, reason {}",
                resp.version.unwrap(),
                &response_code,
                resp.reason.unwrap()
            );
            debug!("Printing response headers:");
            for header in headers.iter().filter(|h| **h != httparse::EMPTY_HEADER) {
                debug!("{}: {}", header.name, String::from_utf8_lossy(header.value));
            }

            buf.advance(n + 4);

            // HTTP code 101: Switching Protocols
            if response_code != 101 && ssl_stream.read_to_end(&mut buf.to_vec()).await.unwrap() == 0
            {
                debug!("Body: {}", String::from_utf8_lossy(&buf).trim());
            }

            if !buf.is_empty() {
                debug!(
                    "Current buffer is not empty?: {}",
                    String::from_utf8_lossy(&buf).trim()
                );
                panic!("Buffer should be empty, are the peer slots full?");
            }
        }

        ssl_stream
    }

    /// Create a request message which wil upgrade the connection between peer and interceptor
    /// The content is trivial. The Session-Signature gets neglected (dummy value 'a')
    /// since we removed the handshake verification check in the rippled source code.
    fn format_upgrade_request_content(pub_key_peer_to: &str) -> String {
        format!(
            "\
            GET / HTTP/1.1\r\n\
            Upgrade: XRPL/2.2\r\n\
            Connection: Upgrade\r\n\
            Connect-As: Peer\r\n\
            Public-Key: {}\r\n\
            Session-Signature: a\r\n\
            \r\n",
            pub_key_peer_to
        )
    }

    /// Handle the connection between 2 peers, while keeping track of the peers' numbers
    /// Returns 2 threads which continuously handle incoming messages
    async fn handle_peer_connections(
        ssl_stream_1: SslStream<TcpStream>,
        ssl_stream_2: SslStream<TcpStream>,
        peer1_port: u16,
        peer2_port: u16,
    ) -> (JoinHandle<()>, JoinHandle<()>) {
        let arc_stream_peer1_0 = Arc::new(Mutex::new(ssl_stream_1));
        let arc_stream_peer2_0 = Arc::new(Mutex::new(ssl_stream_2));

        let arc_stream_peer1_1 = arc_stream_peer1_0.clone();
        let arc_stream_peer2_1 = arc_stream_peer2_0.clone();

        let thread_1 = tokio::spawn(async move {
            loop {
                Self::handle_message(
                    &arc_stream_peer1_0,
                    &arc_stream_peer2_0,
                    peer1_port,
                    peer2_port,
                )
                .await;
            }
        });

        let thread_2 = tokio::spawn(async move {
            loop {
                Self::handle_message(
                    &arc_stream_peer2_1,
                    &arc_stream_peer1_1,
                    peer2_port,
                    peer1_port,
                )
                .await;
            }
        });

        (thread_1, thread_2)
    }

    /// Handles incoming messages from the 'from' stream to the 'to' stream.
    /// Utilizes the controller module to determine new packet contents and action
    async fn handle_message(
        peer_from_stream: &Arc<Mutex<SslStream<TcpStream>>>,
        peer_to_stream: &Arc<Mutex<SslStream<TcpStream>>>,
        peer_from_port: u16,
        peer_to_port: u16,
    ) {
        let mut buf = BytesMut::with_capacity(64 * 1024);
        buf.resize(64 * 1024, 0);
        let size = peer_from_stream
            .lock()
            .await
            .read(buf.as_mut())
            .await
            .expect("Could not read from SSL stream");

        // This is used to make sure we exclude execution time when delaying messages
        let start_time = Instant::now();

        buf.resize(size, 0);
        if size == 0 {
            error!("Current buffer: {}", String::from_utf8_lossy(&buf).trim());
            return;
        }
        let bytes = buf.to_vec();

        // Check if the most significant bit turned on, indicating a compressed message
        if bytes[0] & 0x80 != 0 {
            error!("{:?}", bytes[0]);
            panic!("Received compressed message");
        }

        // Check if any of the 6 most significant bits are turned on, indicating an unknown header
        if bytes[0] & 0xFC != 0 {
            error!("{:?}", bytes[0]);
            panic!("Unknown version header")
        }

        let payload_size = u32::from_be_bytes(bytes[0..4].try_into().unwrap()) as usize;

        if payload_size > 64 * 1024 * 1024 {
            panic!("Message size too large");
        }

        if buf.len() < 6 + payload_size {
            panic!("Buffer is too short");
        }

        let message = bytes[0..(6 + payload_size)].to_vec();

        // TODO: send the message to the controller
        // TODO: use returned information for further execution

        // For now for testing purposes: peer1 gets delayed for 1000ms with a chance of p=0.3
        // Move the current execution to a tokio thread which will delay and then send the message
        if peer_from_port == 60001 && thread_rng().gen_bool(0.3) {
            let peer_to_stream_clone = peer_to_stream.clone();
            let _delay_thread = tokio::spawn(async move {
                Self::delay_execution(peer_from_port, start_time, 1000).await;
                peer_to_stream_clone
                    .lock()
                    .await
                    .write_all(&buf)
                    .await
                    .expect("Could not write to SSL stream");
                debug!(
                    "Forwarded peer message {} -> {}",
                    peer_from_port, peer_to_port
                );
                buf.advance(payload_size + 6);
            });
        }
        // For now: send the raw bytes without processing to the receiver
        else {
            peer_to_stream
                .lock()
                .await
                .write_all(&message)
                .await
                .expect("Could not write to SSL stream");
            debug!(
                "Forwarded peer message {} -> {}",
                peer_from_port, peer_to_port
            );
            buf.advance(payload_size + 6);
        }
    }

    /// Delay execution with respect to a defined starting time
    async fn delay_execution(peer_id: u16, start_time: Instant, ms: u64) {
        let elapsed_time = start_time.elapsed();
        let delay_duration = Duration::from_millis(ms) - elapsed_time;

        debug!("Delaying peer {} for {} ms", peer_id, ms);

        if delay_duration > Duration::new(0, 0) {
            tokio::time::sleep(delay_duration).await;
        }
    }
}
