//! This module is responsible for setting up connections between peers.

use bytes::{Buf, BytesMut};
use log::{debug, error};
use openssl::ssl::{Ssl, SslContext, SslMethod};
use std::net::{IpAddr, SocketAddr};
use std::pin::Pin;
use std::str::FromStr;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_openssl::SslStream;

/// Struct that represents the object that connects peers with each other.
#[derive(Clone)]
pub struct PeerConnector {
    /// The IP address of every peer. Only the ports of the peers differ.
    pub ip_addr: String,
}

impl PeerConnector {
    /// Initializes a new PeerConnector.
    ///
    /// # Parameters
    /// * 'ip_addr' - the IP address of all peers.
    pub fn new(ip_addr: String) -> Self {
        Self { ip_addr }
    }

    /// Connects two peers with each other. Returns both halves of the connection, so the interceptor is in between.
    ///
    /// # Parameters
    /// * 'port_peer_1' - the port of the first peer.
    /// * 'port_peer_2' - the port of the second peer.
    /// * 'pub_key_peer_1' - the public key of the first peer.
    /// * 'pub_key_peer_2' - the public key of the second peer.
    pub async fn connect_peers(
        &self,
        port_peer_1: u16,
        port_peer_2: u16,
        pub_key_peer_1: &str,
        pub_key_peer_2: &str,
    ) -> (SslStream<TcpStream>, SslStream<TcpStream>) {
        let connection_half_1 =
            Self::setup_connection_half(self.ip_addr.as_str(), port_peer_1, pub_key_peer_2).await;
        let connection_half_2 =
            Self::setup_connection_half(self.ip_addr.as_str(), port_peer_2, pub_key_peer_1).await;
        (connection_half_1, connection_half_2)
    }

    /// Sets up a connection half from a peer to another peer.
    /// Connects to the peer at ip:port.
    /// We pretend to be the other peer with its public key.
    /// This way we can intercept the connection.
    ///
    /// # Parameters
    /// * 'ip' - the ip to which we connect to.
    /// * 'port' - the port to which we connect to.
    /// * 'pub_key_of_peer_we_pretend_to_be' - the public key of the peer we pretend to be.
    ///
    /// # Panics
    /// * If an error occurred while creating and connecting the SslStream.
    /// * If an error occurred while reading or writing to/from the SslStream.
    /// * If the response of the upgrade request is invalid.
    async fn setup_connection_half(
        ip: &str,
        port: u16,
        pub_key_of_peer_we_pretend_to_be: &str,
    ) -> SslStream<TcpStream> {
        let mut ssl_stream = Self::create_and_connect_ssl_stream(ip, port).await;
        let content = Self::format_upgrade_request_content(pub_key_of_peer_we_pretend_to_be);
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

        Self::check_upgrade_request_response(buf);

        ssl_stream
    }

    /// This method checks given a buffered HTTP response, whether it is a valid 101 switching protocol response.
    ///
    /// # Panics
    /// * If it could not parse the response.
    /// * If it received a partial message.
    /// * If it could not separate the HTTP headers from the body.
    /// * If the response code is not '101' as expected to be.
    fn check_upgrade_request_response(mut buffered_response: BytesMut) {
        if let Some(n) = buffered_response.windows(4).position(|x| x == b"\r\n\r\n") {
            let mut headers = [httparse::EMPTY_HEADER; 32];
            let mut response = httparse::Response::new(&mut headers);

            let status = response
                .parse(&buffered_response[0..n + 4])
                .expect("Response parse failed.");

            if status.is_partial() {
                panic!("Did not fully parse the response.");
            }

            let response_status_code = response.code.unwrap();

            debug!(
                "Peer Handshake Response: version {}, status {}, reason {}",
                response.version.unwrap(),
                &response_status_code,
                response.reason.unwrap()
            );

            debug!("Printing response headers:");
            for header in headers.iter().filter(|h| **h != httparse::EMPTY_HEADER) {
                debug!("{}: {}", header.name, String::from_utf8_lossy(header.value));
            }

            buffered_response.advance(n + 4);

            // HTTP code 101: Switching Protocols
            if response_status_code != 101 {
                panic!(
                    "Response status code expected to be 101 but was: {}\n\
                    Body of the response: {}",
                    response_status_code,
                    String::from_utf8_lossy(&buffered_response).trim()
                );
            }

            if !buffered_response.is_empty() {
                debug!(
                    "Switching protocol response has an (unexpected) body: {}",
                    String::from_utf8_lossy(&buffered_response).trim()
                );
            }
        } else {
            panic!("Could not separate HTTP headers from body. Response is invalid.")
        }
    }

    /// Creates a SslStream and connects to the specified IP address + port.
    ///
    /// # Parameters
    /// * 'ip' - the IP address to which a connection should be made.
    /// * 'port' - the port to which a connection should be made.
    ///
    /// # Panics
    /// * If the ip:port specified is invalid.
    /// * If the SslStream could not be created or connected to.
    async fn create_and_connect_ssl_stream(ip: &str, port: u16) -> SslStream<TcpStream> {
        let socket_address = SocketAddr::new(IpAddr::from_str(ip).unwrap(), port);
        let tcp_stream = TcpStream::connect(socket_address).await.unwrap();

        tcp_stream.set_nodelay(true).expect("Set nodelay failed");
        let ssl_context = SslContext::builder(SslMethod::tls()).unwrap().build();
        let ssl_session = Ssl::new(&ssl_context).unwrap();
        let mut ssl_stream = SslStream::<TcpStream>::new(ssl_session, tcp_stream).unwrap();
        SslStream::connect(Pin::new(&mut ssl_stream))
            .await
            .expect("SSL connection failed");

        ssl_stream
    }

    /// Creates a request message which wil upgrade the connection between peer and interceptor
    /// The content is trivial. The Session-Signature gets neglected (dummy value 'a')
    /// since we removed the handshake verification check in the rippled source code.
    ///
    /// # Parameters
    /// * 'pub_key_peer_to' - the public key to be filled in into the request.
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

#[cfg(test)]
mod unit_tests {
    use crate::peer_connector::PeerConnector;
    use bytes::BytesMut;

    #[test]
    // #[coverage(off)]  // Only available in nightly build, don't forget to uncomment #![feature(coverage_attribute)] on line 1 of main
    fn peer_connector_new_test() {
        let peer_connector = PeerConnector::new("127.0.0.1".to_string());
        assert_eq!(peer_connector.ip_addr, "127.0.0.1".to_string());
    }

    #[test]
    // #[coverage(off)]  // Only available in nightly build, don't forget to uncomment #![feature(coverage_attribute)] on line 1 of main
    fn upgrade_request_test() {
        let expected = String::from(
            "\
            GET / HTTP/1.1\r\n\
            Upgrade: XRPL/2.2\r\n\
            Connection: Upgrade\r\n\
            Connect-As: Peer\r\n\
            Public-Key: 123456789abcdefg\r\n\
            Session-Signature: a\r\n\
            \r\n",
        );
        assert_eq!(
            expected,
            PeerConnector::format_upgrade_request_content("123456789abcdefg"),
        )
    }

    #[test]
    // #[coverage(off)]  // Only available in nightly build, don't forget to uncomment #![feature(coverage_attribute)] on line 1 of main
    fn check_upgrade_request_response_no_panic() {
        let response = b"\
            HTTP/1.1 101 Switching Protocol\r\n
            Connection: Upgrade\r\n\
            Upgrade: XRPL/2.2\r\n
            Connect-As: Peer\r\n
            Server: rippled-2.1.1\r\n
            Crawl: private\r\n
            X-Protocol-Ctl:\r\n
            Network-Time: 770391649\r\n
            Public-Key: n9M1Fh52PBMSrEjjs8Y64EmU8hfVzb29BBDaXoVNS3AaC1gM19CP\r\n
            Session-Signature: MEUCIQCBsA3JThSv4geQ67ZlrLvBZGO0wiWWU5pfDsiKalvwKQIgb6CuAHAYnxGf4MYB4Jgsbox4of5GxT4IbRPWablVQ9w=\r\n
            Instance-Cookie: 16110088623413850902\r\n
            Closed-Ledger: 2D7DE9661AADBCDC6DD6630F0C616F5BE29803A5A5DC31486DD65E0F6A79DDB1\r\n
            Previous-Ledger: 0000000000000000000000000000000000000000000000000000000000000000\r\n\r\n
        ";

        let mut buffer = BytesMut::new();
        buffer.extend_from_slice(response);
        PeerConnector::check_upgrade_request_response(buffer);
    }

    #[test]
    // #[coverage(off)]  // Only available in nightly build, don't forget to uncomment #![feature(coverage_attribute)] on line 1 of main
    #[should_panic(expected = "Response parse failed.")]
    fn check_upgrade_request_response_invalid_request() {
        let mut buffer = BytesMut::new();
        buffer.extend_from_slice(b"garbage\r\n\r\n");
        PeerConnector::check_upgrade_request_response(buffer);
    }

    #[test]
    // #[coverage(off)]  // Only available in nightly build, don't forget to uncomment #![feature(coverage_attribute)] on line 1 of main
    #[should_panic(expected = "Could not separate HTTP headers from body. Response is invalid.")]
    fn check_upgrade_request_response_invalid_response() {
        let mut buffer = BytesMut::new();
        let data = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];
        buffer.extend_from_slice(&data);
        PeerConnector::check_upgrade_request_response(buffer);
    }

    #[test]
    // #[coverage(off)]  // Only available in nightly build, don't forget to uncomment #![feature(coverage_attribute)] on line 1 of main
    #[should_panic(
        expected = "Response status code expected to be 101 but was: 404\nBody of the response: <body message>"
    )]
    fn check_upgrade_request_response_wrong_status_code() {
        let mut buf = BytesMut::new();
        buf.extend_from_slice(b"HTTP/1.1 404 Not Found\r\n\r\n<body message>");

        PeerConnector::check_upgrade_request_response(buf);
    }
}
