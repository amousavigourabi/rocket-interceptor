use bytes::{Buf, BytesMut};
use log::{debug, error};
use openssl::ssl::{Ssl, SslContext, SslMethod};
use std::net::{IpAddr, SocketAddr};
use std::pin::Pin;
use std::str::FromStr;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_openssl::SslStream;

#[derive(Clone)]
pub struct PeerConnector {
    pub ip_addr: String,
}

impl PeerConnector {
    pub fn new(ip_addr: String) -> Self {
        Self { ip_addr }
    }

    pub async fn connect_peers(
        &self,
        peer1_port: u16,
        peer2_port: u16,
        pub_key1: &str,
        pub_key2: &str,
    ) -> (SslStream<TcpStream>, SslStream<TcpStream>) {
        let ssl_stream_1 =
            Self::create_ssl_stream(self.ip_addr.as_str(), peer1_port, pub_key2).await;
        let ssl_stream_2 =
            Self::create_ssl_stream(self.ip_addr.as_str(), peer2_port, pub_key1).await;
        (ssl_stream_1, ssl_stream_2)
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

    /// Creates a request message which wil upgrade the connection between peer and interceptor
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
}
