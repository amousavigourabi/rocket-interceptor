use std::io;
use std::io::Read;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::rc::Rc;
use std::str::FromStr;
use std::sync::Arc;

use bytes::{Buf, BytesMut};
use log::*;
use openssl::base64;
use openssl::ssl::{Ssl, SslContext, SslMethod};
use secp256k1::{Message as CryptoMessage, Secp256k1, SecretKey};
use sha2::{Digest, Sha512};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::macros::support::Pin;
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio_openssl::SslStream;

async fn connect_to_peer(ip: &str, public_key: &str) -> SslStream<TcpStream> {
    let proxy_address = SocketAddr::new(IpAddr::from_str(ip).unwrap(), 51235);

    let stream = match TcpStream::connect(proxy_address).await {
        Ok(tcp_stream) => tcp_stream,
        Err(e) => panic!("{}", e),
    };
    stream.set_nodelay(true).expect("Set nodelay failed");

    let ctx = SslContext::builder(SslMethod::tls()).unwrap().build();
    let ssl = Ssl::new(&ctx).unwrap();
    let mut ssl_stream = SslStream::<TcpStream>::new(ssl, stream).unwrap();
    SslStream::connect(Pin::new(&mut ssl_stream))
        .await
        .expect("Ssl connection failed");

    let content = format!(
        "\
            GET / HTTP/1.1\r\n\
            Upgrade: XRPL/2.2\r\n\
            Connection: Upgrade\r\n\
            Connect-As: Peer\r\n\
            Public-Key: {}\r\n\
            Session-Signature: a\r\n\
            \r\n",
        public_key
    );
    ssl_stream
        .write_all(content.as_bytes())
        .await
        .expect("Unable to write during handshake");

    let mut buf = BytesMut::new();
    let mut vec = vec![0; 4096];
    let size = ssl_stream
        .read(&mut vec)
        .await
        .expect("Unable to read during handshake");
    vec.resize(size, 0);
    buf.extend_from_slice(&vec);

    if size == 0 {
        error!("Current buffer: {}", String::from_utf8_lossy(&buf).trim());
        panic!("socket closed");
    }

    if let Some(n) = buf.windows(4).position(|x| x == b"\r\n\r\n") {
        let mut headers = [httparse::EMPTY_HEADER; 32];
        let mut resp = httparse::Response::new(&mut headers);
        let status = resp.parse(&buf[0..n + 4]).expect("response parse success");
        if status.is_partial() {
            panic!("Invalid headers");
        }

        let response_code = resp.code.unwrap();
        debug!(
            "Peer Handshake Response: version {}, status {}, reason {} \nheaders:\n",
            resp.version.unwrap(),
            resp.code.unwrap(),
            resp.reason.unwrap()
        );
        for header in headers.iter().filter(|h| **h != httparse::EMPTY_HEADER) {
            debug!("{}: {}", header.name, String::from_utf8_lossy(header.value));
        }

        buf.advance(n + 4);

        if response_code != 101 {
            loop {
                if ssl_stream.read_to_end(&mut buf.to_vec()).await.unwrap() == 0 {
                    debug!("Body: {}", String::from_utf8_lossy(&buf).trim());
                }
                break;
            }
        }

        if !buf.is_empty() {
            debug!(
                "Current buffer is not empty?: {}",
                String::from_utf8_lossy(&buf).trim()
            );
            panic!("buffer should be empty");
        }
    }
    ssl_stream
}

async fn peer_forward_msg(
    from: Arc<Mutex<SslStream<TcpStream>>>,
    to: Arc<Mutex<SslStream<TcpStream>>>,
) {
    let mut buf = BytesMut::with_capacity(64 * 1024);
    buf.resize(64 * 1024, 0);
    let size = from
        .lock()
        .await
        .read(buf.as_mut())
        .await
        .expect("Unable to read from ssl stream");
    buf.resize(size, 0);
    if size == 0 {
        error!("Current buffer: {}", String::from_utf8_lossy(&buf).trim());
        return;
    }
    let mut bytes = buf.to_vec();
    if bytes[0] & 0x80 != 0 {
        error!("{:?}", bytes[0]);
        panic!("Received compressed message");
    }

    if bytes[0] & 0xFC != 0 {
        error!("Unknown version header");
    }
    to.lock()
        .await
        .write_all(&buf)
        .await
        .expect("Could not write");
}

async fn handle_conn(node1: SslStream<TcpStream>, node2: SslStream<TcpStream>) {
    let arc_stream1_0 = Arc::new(Mutex::new(node1));
    let arc_stream2_0 = Arc::new(Mutex::new(node2));

    let arc_stream1_1 = Arc::clone(&arc_stream1_0);
    let arc_stream2_1 = Arc::clone(&arc_stream2_0);

    let t1 = tokio::spawn(async move {
        loop {
            peer_forward_msg(arc_stream1_0.clone(), arc_stream2_0.clone()).await;
        }
    });

    let t2 = tokio::spawn(async move {
        loop {
            peer_forward_msg(arc_stream1_1.clone(), arc_stream2_1.clone()).await;
        }
    });

    t1.await.expect("thread 1 failed.");
    t2.await.expect("thread 2 failed.");
}

#[tokio::main]
async fn main() -> io::Result<()> {
    let n1_public_key = "n9JAC3PDvNcLkR6uCRWvrBMQDs4UFR2UqhL5yU8xdDcdhTfqUxci";
    let n2_public_key = "n9KWgTyg72yf1AdDoEM3GaDFUPaZNK3uf66uoVpMZeNnGegC9yz2";

    let ssl_stream1 = connect_to_peer("172.18.0.3", n2_public_key).await;
    let ssl_stream2 = connect_to_peer("172.18.0.2", n1_public_key).await;

    handle_conn(ssl_stream1, ssl_stream2).await;
    Ok(())
}
