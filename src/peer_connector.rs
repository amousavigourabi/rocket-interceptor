use crate::packet_client;
use crate::packet_client::PacketClient;
use bytes::{Buf, BytesMut};
use log::{debug, error};
use openssl::ssl::{Ssl, SslContext, SslMethod};
use std::net::{IpAddr, SocketAddr};
use std::pin::Pin;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt, ReadHalf, WriteHalf};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio_openssl::SslStream;

#[derive(Clone)]
pub struct PeerConnector {
    pub ip_addr: String,
}

impl PeerConnector {
    pub fn new(ip_addr: String) -> Self {
        Self { ip_addr }
    }
    /// Connect 2 peers using their id's and public keys.
    /// Established SSL streams between the peers.
    /// Returns 2 threads which handle the messages sent over the streams.
    pub async fn connect_peers(
        self,
        client: Arc<Mutex<packet_client::PacketClient>>,
        peer1_port: u16,
        peer2_port: u16,
        pub_key1: &str,
        pub_key2: &str,
    ) -> (JoinHandle<()>, JoinHandle<()>) {
        let ssl_stream_1 =
            Self::create_ssl_stream(self.ip_addr.as_str(), peer1_port, pub_key2).await;
        let ssl_stream_2 =
            Self::create_ssl_stream(self.ip_addr.as_str(), peer2_port, pub_key1).await;
        Self::handle_peer_connections(client, ssl_stream_1, ssl_stream_2, peer1_port, peer2_port)
            .await
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
        client: Arc<Mutex<PacketClient>>,
        ssl_stream_1: SslStream<TcpStream>,
        ssl_stream_2: SslStream<TcpStream>,
        peer1_port: u16,
        peer2_port: u16,
    ) -> (JoinHandle<()>, JoinHandle<()>) {
        let (mut r1, mut w1) = tokio::io::split(ssl_stream_1);
        let (mut r2, mut w2) = tokio::io::split(ssl_stream_2);

        let client1 = Arc::clone(&client);
        let thread_1 = tokio::spawn(async move {
            loop {
                Self::handle_message(client1.as_ref(), &mut r1, &mut w2, peer1_port, peer2_port)
                    .await;
            }
        });

        let client2 = Arc::clone(&client);
        let thread_2 = tokio::spawn(async move {
            loop {
                Self::handle_message(client2.as_ref(), &mut r2, &mut w1, peer1_port, peer2_port)
                    .await;
            }
        });

        (thread_1, thread_2)
    }

    /// Handles incoming messages from the 'from' stream to the 'to' stream.
    /// Utilizes the controller module to determine new packet contents and action
    async fn handle_message(
        client: &Mutex<PacketClient>,
        peer_from_stream: &mut ReadHalf<SslStream<TcpStream>>,
        peer_to_stream: &mut WriteHalf<SslStream<TcpStream>>,
        peer_from_port: u16,
        peer_to_port: u16,
    ) {
        let mut buf = BytesMut::with_capacity(64 * 1024);
        buf.resize(64 * 1024, 0);
        let size = peer_from_stream
            .read(buf.as_mut())
            .await
            .expect("Could not read from SSL stream");

        let read_moment = Instant::now();

        buf.resize(size, 0);
        if size == 0 {
            panic!("Current buffer: {}", String::from_utf8_lossy(&buf).trim());
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
            error!("Buffer is too short");
            return;
        }

        let message = bytes[0..(6 + payload_size)].to_vec();
        let response = client
            .lock()
            .await
            .send_packet(message, u32::from(peer_from_port), u32::from(peer_to_port))
            .await
            .unwrap();

        match response.action {
            0 => (),
            u32::MAX => {
                debug!(
                    "Dropping a message sent from {} to {}",
                    peer_from_port, peer_to_port
                );
                return;
            }
            delay => {
                debug!(
                    "Delaying a message sent from {} to {} for {} ms",
                    peer_from_port, peer_to_port, delay
                );
                Self::delay_execution(read_moment, delay as u64).await;
            }
        }

        peer_to_stream
            .write_all(&response.data)
            .await
            .expect("Could not write to SSL stream");
        debug!(
            "Forwarded peer message {} -> {}",
            peer_from_port, peer_to_port
        );
        buf.advance(6 + payload_size);
    }

    /// Delay execution with respect to a defined starting time
    async fn delay_execution(start_time: Instant, ms: u64) {
        let elapsed_time = start_time.elapsed();
        let total_delay = Duration::from_millis(ms);
        if(elapsed_time < total_delay) {
            let delay_duration = total_delay - elapsed_time;
            tokio::time::sleep(delay_duration).await;
        }
    }
}
