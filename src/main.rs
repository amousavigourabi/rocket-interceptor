use std::io;
use std::io::Read;
use bytes::{Buf, BytesMut};
use log::*;
use openssl::ssl::{Ssl, SslContext, SslMethod};
use secp256k1::{Message as CryptoMessage, Secp256k1, SecretKey};
use sha2::{Digest, Sha512};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::Mutex;
use std::rc::Rc;
use openssl::base64;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::macros::support::Pin;
use tokio::net::TcpStream;
use tokio::task::JoinHandle;
use tokio_openssl::SslStream;

#[tokio::main]
async fn main() -> io::Result<()> {
    let n1_public_key = "n9JAC3PDvNcLkR6uCRWvrBMQDs4UFR2UqhL5yU8xdDcdhTfqUxci";
    let n2_public_key = "n9KWgTyg72yf1AdDoEM3GaDFUPaZNK3uf66uoVpMZeNnGegC9yz2";
    //let private_key = "sskExrvcMP7YGDnS5ub8YCgEBcRc1";


    let ssl_stream_1 = Arc::new(Mutex::new(connect_to_peer("172.18.0.3", n2_public_key).await));
    let ssl_stream_2 = Arc::new(Mutex::new(connect_to_peer("172.18.0.2", n1_public_key).await));

    let ssl_stream_1_1 = Arc::clone(&ssl_stream_1);
    let ssl_stream_2_1 = Arc::clone(&ssl_stream_2);

    let t1 = tokio::spawn(async move {
        loop {
            let mut buf = BytesMut::with_capacity(64 * 1024);
            buf.resize(64 * 1024, 0);
            let size = ssl_stream_1.lock().await
                .read(buf.as_mut())
                .await
                .expect("Unable to read from ssl stream");
            buf.resize(size, 0);
            if size == 0 {
                error!(
                    "Current buffer: {}",
                    String::from_utf8_lossy(&buf).trim()
                );
            }
            let mut bytes = buf.to_vec();
            if bytes[0] & 0x80 != 0 {
                error!("{:?}", bytes[0]);
                panic!("Received compressed message");
            }

            if bytes[0] & 0xFC != 0 {
                error!("Unknown version header");
            }

            //let payload_size = u32::from_be_bytes(bytes[0..4].try_into().unwrap()) as usize;

            //if payload_size > 64 * 1024 * 1024 {
            //    panic!("Message size too large");
            //}

            //if buf.len() < 6 + payload_size {
            //    break;
            //}

            // Send received message to scheduler
            //let message = bytes[0..(6 + payload_size)].to_vec();
            //println!("Current buffer: {}", String::from_utf8_lossy(&buf).trim());
            ssl_stream_2.lock().await.write_all(&buf).await.expect("Could not write");
            println!("Sent to 2");
        }
    });
    let t2 = tokio::spawn(async move {
        loop {
            let mut buf = BytesMut::with_capacity(64 * 1024);
            buf.resize(64 * 1024, 0);
            let size = ssl_stream_2_1.lock().await
                .read(buf.as_mut())
                .await
                .expect("Unable to read from ssl stream");
            buf.resize(size, 0);
            if size == 0 {
                error!(
                    "Current buffer: {}",
                    String::from_utf8_lossy(&buf).trim()
                );
            }
            let mut bytes = buf.to_vec();
            if bytes[0] & 0x80 != 0 {
                error!("{:?}", bytes[0]);
                panic!("Received compressed message");
            }

            if bytes[0] & 0xFC != 0 {
                error!("Unknow version header");
            }

            //let payload_size = u32::from_be_bytes(bytes[0..4].try_into().unwrap()) as usize;

            //if payload_size > 64 * 1024 * 1024 {
            //    panic!("Message size too large");
            //}

            //if buf.len() < 6 + payload_size {
            //    break;
            //}

            // Send received message to scheduler
            //let message = bytes[0..(6 + payload_size)].to_vec();
            //println!("Current buffer: {}", String::from_utf8_lossy(&buf).trim());
            ssl_stream_1_1.lock().await.write_all(&buf).await.expect("Could not write");
            println!("Sent to 1");
        }
    });
    t1.await.expect("Could not join t1");
    t2.await.expect("Could not join t2");
    Ok(())
}

async fn connect_to_peer(ip: &str, public_key: &str) -> SslStream<TcpStream> {
    let proxy_address = SocketAddr::new(
        IpAddr::from_str(ip).unwrap(),
        51235,
    );
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
    println!("Body: {}", String::from_utf8_lossy(&buf).trim());
    let mut vec = vec![0; 4096];
    let size = ssl_stream
        .read(&mut vec)
        .await
        .expect("Unable to read during handshake");
    vec.resize(size, 0);
    buf.extend_from_slice(&vec);

    if size == 0 {
        error!(
                    "Current buffer: {}",
                    String::from_utf8_lossy(&buf).trim()
                );
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
        println!(
            "Response: version {}, code {}, reason {}",
            resp.version.unwrap(),
            resp.code.unwrap(),
            resp.reason.unwrap()
        );
        for header in headers.iter().filter(|h| **h != httparse::EMPTY_HEADER) {
            println!("{}: {}", header.name, String::from_utf8_lossy(header.value));
        }

        buf.advance(n + 4);

        if response_code != 101 {
            loop {
                if ssl_stream.read_to_end(&mut buf.to_vec()).await.unwrap() == 0 {
                    println!("Body: {}", String::from_utf8_lossy(&buf).trim());
                }
                break;
            }
        }

        if !buf.is_empty() {
            println!(
                "Current buffer is not empty?: {}",
                String::from_utf8_lossy(&buf).trim()
            );
            panic!("buffer should be empty");
        }
    }
    ssl_stream
}