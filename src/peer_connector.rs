//! This module is responsible for setting up connections between peers.

use base64::engine::general_purpose;
use base64::Engine;
use basex_rs::{BaseX, ALPHABET_RIPPLE};
use bytes::{Buf, BytesMut};
use log::{debug, error, info};
use openssl::sha::Sha512;
use openssl::ssl::{Ssl, SslContext, SslMethod};
use secp256k1::{Message as CryptoMessage, Secp256k1, SecretKey};
use std::pin::Pin;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_openssl::SslStream;

/// Struct that represents the object that connects peers with each other.
#[derive(Clone)]
pub struct PeerConnector {
    /// The port of every peer. Only the hostnames of the peers differ. 
    /// TODO: Stupid decision jeez
    pub port: u16,
}

impl PeerConnector {
    /// Initializes a new PeerConnector.
    ///
    /// # Parameters
    /// * 'ip_addr' - the IP address of all peers.
    pub fn new(port: u16) -> Self {
        Self { port }
    }

    /// Connects two peers with each other. Returns both halves of the connection, so the interceptor is in between.
    ///
    /// # Parameters
    /// * 'port_peer_1' - the port of the first peer.
    /// * 'port_peer_2' - the port of the second peer.
    /// * 'pub_key_peer_1' - the public key of the first peer.
    /// * 'pub_key_peer_2' - the public key of the second peer.
    /// * 'seed_peer_1' - the validation seed of the first peer.
    /// * 'seed_peer_2' - the validation seed of the second peer.
    pub async fn connect_peers(
        &self,
        hostname_peer_1: &str,
        hostname_peer_2: &str,
        pub_key_peer_1: &str,
        pub_key_peer_2: &str,
        seed_peer_1: &str,
        seed_peer_2: &str,
    ) -> (SslStream<TcpStream>, SslStream<TcpStream>) {
        let connection_half_1 = Self::setup_connection_half(
            hostname_peer_1, 
            self.port,
            pub_key_peer_2,
            seed_peer_2,
        )
        .await;
        let connection_half_2 = Self::setup_connection_half(
            hostname_peer_2,
            self.port,
            pub_key_peer_1,
            seed_peer_1,
        )
        .await;
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
    /// * 'initiator_public_key' - the public key of the peer we pretend to be.
    /// * 'initiator_seed' - the validation seed of the peer we pretend to be.
    ///
    /// # Panics
    /// * If an error occurred while creating and connecting the SslStream.
    /// * If an error occurred while reading or writing to/from the SslStream.
    /// * If the response of the upgrade request is invalid.
    async fn setup_connection_half(
        hostname: &str,
        port: u16,
        initiator_public_key: &str,
        initiator_seed: &str,
    ) -> SslStream<TcpStream> {
        let mut ssl_stream =
            Self::create_and_connect_ssl_stream(hostname, port, initiator_public_key, initiator_seed)
                .await;

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
                .expect("Parsing response failed.");

            if status.is_partial() {
                panic!("Could not fully parse the response.");
            }

            let response_status_code = response.code.unwrap();

            debug!(
                "Peer Handshake Response: version: {}, status: {}, reason: {}",
                response.version.unwrap(),
                &response_status_code,
                response.reason.unwrap()
            );

            debug!("Response headers:");
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
    /// * 'public_key' - the public key of the node initiating the connection.
    /// * 'seed' - the validation seed of the node initiating the connection.
    ///
    /// # Panics
    /// * If the ip:port specified is invalid.
    /// * If the SslStream could not be created or connected to.
    async fn create_and_connect_ssl_stream(
        hostname: &str,
        port: u16,
        public_key: &str,
        seed: &str,
    ) -> SslStream<TcpStream> {
        let address = format!("{}:{}", hostname, port);
        println!("Connecting to {}...", address);
        let tcp_stream = TcpStream::connect(address).await.unwrap();

        tcp_stream
            .set_nodelay(true)
            .expect("Enable TCP_NODELAY failed.");
        let ssl_context = SslContext::builder(SslMethod::tls()).unwrap().build();
        let ssl_session = Ssl::new(&ssl_context).unwrap();
        let mut ssl_stream = SslStream::<TcpStream>::new(ssl_session, tcp_stream).unwrap();
        SslStream::connect(Pin::new(&mut ssl_stream))
            .await
            .expect("SSL connection failed.");

        // The following block of code is responsible for computing the Session-Signature
        // for the Handshake, which is required to establish a connection between two nodes.
        // See https://github.com/XRPLF/rippled/blob/f64cf9187affd69650907d0d92e097eb29693945/src/xrpld/overlay/detail/Handshake.cpp#L199-L203
        // for the original implementation by the XRPLF.
        let ssl_ref = ssl_stream.ssl();
        let mut buf = vec![0; 1024];

        // Get the contents of the last message sent to the peer.
        let mut size = ssl_ref.finished(&mut buf[..]);
        if size > buf.len() {
            buf.resize(size, 0);
            size = ssl_ref.finished(&mut buf[..]);
        }
        // Hash the finished message
        let mut ctx_sha512_message1 = Sha512::new();
        ctx_sha512_message1.update(&buf[..size]);
        let message1_hash = &ctx_sha512_message1.finish();
        debug!("Finished message SHA512: {:?}", message1_hash);

        // Get the contents of the last received message from the peer.
        let mut size = ssl_ref.peer_finished(&mut buf[..]);
        if size > buf.len() {
            buf.resize(size, 0);
            size = ssl_ref.peer_finished(&mut buf[..]);
        }
        // Hash the received message
        let mut ctx_sha512_message2 = Sha512::new();
        ctx_sha512_message2.update(&buf[..size]);
        let message2_hash = &ctx_sha512_message2.finish();
        debug!("Received message SHA512: {:?}", message2_hash);

        // XOR the contents of both finished messages
        let message_xor = message1_hash
            .iter()
            .zip(message2_hash.iter())
            .map(|(a, b)| a ^ b)
            .collect::<Vec<u8>>();
        debug!("XOR of messages: {:?}", message_xor);

        let mut ctx_sha512_xor = Sha512::new();
        ctx_sha512_xor.update(&message_xor[..]);
        let xor_hash = ctx_sha512_xor.finish();
        let msg = CryptoMessage::from_digest_slice(&xor_hash[0..32]).unwrap();

        let mut seed_bytes = BaseX::with_alphabet(ALPHABET_RIPPLE)
            .from_bs58(&String::from(seed))
            .unwrap();
        let mut ctx_sha512_seed = Sha512::new();

        // Set last 4 bytes (bytes 18-21) to 0
        // These bytes are the "Root key sequence", and signify how many times the key had to
        // be regenerated before being a valid secp256k1 secret key. Anything over 0 is highly
        // unlikely, thus hardcoded here. If not set to zero, they seem to take on a random value,
        // which causes the signature to become invalid.
        // https://xrpl.org/docs/concepts/accounts/cryptographic-keys#secp256k1-key-derivation
        seed_bytes[17] = 0u8;
        seed_bytes[18] = 0u8;
        seed_bytes[19] = 0u8;
        seed_bytes[20] = 0u8;

        ctx_sha512_seed.update(&seed_bytes[1..]);
        let seed_hash = ctx_sha512_seed.finish();
        let secp256k1_ctx = Secp256k1::new();
        let sk = SecretKey::from_slice(&seed_hash[..32]).unwrap();
        let sig = secp256k1_ctx.sign_ecdsa(&msg, &sk).serialize_der();
        let b64sig = general_purpose::STANDARD.encode(sig);

        let content = Self::format_upgrade_request_content(public_key, b64sig.as_str());
        ssl_stream
            .write_all(content.as_bytes())
            .await
            .expect("Could not send XRPL handshake request.");

        ssl_stream
    }

    /// Creates a request message which wil upgrade the connection between peer and interceptor
    /// The content is trivial. The Session-Signature gets neglected (dummy value 'a')
    /// since we removed the handshake verification check in the rippled source code.
    ///
    /// # Parameters
    /// * 'public_key' - the public key to be filled in into the request.
    /// * 'base64_sig' - the base64 encoded session signature to be filled in into the request.
    fn format_upgrade_request_content(public_key: &str, base64_sig: &str) -> String {
        format!(
            "\
            GET / HTTP/1.1\r\n\
            Upgrade: XRPL/2.2\r\n\
            Connection: Upgrade\r\n\
            Connect-As: Peer\r\n\
            Public-Key: {}\r\n\
            Session-Signature: {}\r\n\
            \r\n",
            public_key, base64_sig
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
        let peer_connector = PeerConnector::new(51235);
        assert_eq!(peer_connector.port, 51235);
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
            Session-Signature: 123456789abcdefg\r\n\
            \r\n",
        );
        assert_eq!(
            expected,
            PeerConnector::format_upgrade_request_content("123456789abcdefg", "123456789abcdefg"),
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
            Session-Signature: MEUCIQCBsA3JThSv4geQ67ZlrLvBZGO0wiWWU5pfDsiKalvwKQIgb6CuAHAYnxGf4MYB4Jgsbox4of5GxT4IbRPWablVQ9w=\r\n\
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
    #[should_panic(expected = "Parsing response failed.")]
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
