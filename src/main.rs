use std::io;
use std::net::{IpAddr, SocketAddr};
use std::str::FromStr;
use std::sync::Arc;

use bytes::{Buf, BytesMut};
use log::*;
use openssl::ssl::{Ssl, SslContext, SslMethod};
use std::env::current_dir;
use std::env::set_var;
use std::process::{Command, Stdio};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::macros::support::Pin;
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio::time::sleep;
use tokio_openssl::SslStream;

async fn connect_to_peer(ip: &str, port: u16, public_key: &str) -> SslStream<TcpStream> {
    let proxy_address = SocketAddr::new(IpAddr::from_str(ip).unwrap(), port);

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
            "Peer Handshake Response: version {}, status {}, reason {}",
            resp.version.unwrap(),
            resp.code.unwrap(),
            resp.reason.unwrap()
        );
        for header in headers.iter().filter(|h| **h != httparse::EMPTY_HEADER) {
            debug!("Printing response headers:");
            debug!("{}: {}", header.name, String::from_utf8_lossy(header.value));
        }

        buf.advance(n + 4);

        if response_code != 101 && ssl_stream.read_to_end(&mut buf.to_vec()).await.unwrap() == 0 {
            debug!("Body: {}", String::from_utf8_lossy(&buf).trim());
        }

        if !buf.is_empty() {
            debug!(
                "Current buffer is not empty?: {}",
                String::from_utf8_lossy(&buf).trim()
            );
            panic!("buffer should be empty, are the slots full?");
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
    let bytes = buf.to_vec();
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
            debug!("forwarded 1->2")
        }
    });

    let t2 = tokio::spawn(async move {
        loop {
            peer_forward_msg(arc_stream2_1.clone(), arc_stream1_1.clone()).await;
            debug!("forwarded 2->1")
        }
    });

    t1.await.expect("thread 1 failed.");
    t2.await.expect("thread 2 failed.");
}

async fn start_container(name: &str, port: u16) {
    debug!("starting docker container: {}", name);
    Command::new("docker")
        .arg("run")
        .arg("-d")
        .arg("-p")
        .arg(format!("{}:51235", port))
        .arg("--name")
        .arg(name)
        .arg("--mount")
        .arg(format!(
            "type=bind,source={}/network/{}/config,target=/config",
            current_dir().unwrap().to_str().unwrap(),
            name
        ))
        .arg("isvanloon/rippled-no-sig-check")
        .stdout(Stdio::null())
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("failed to start docker container");

    sleep(Duration::from_secs(2)).await;
}

#[tokio::main]
async fn main() -> io::Result<()> {
    set_var("RUST_LOG", "DEBUG");
    env_logger::init();

    let n1_public_key = "n9JAC3PDvNcLkR6uCRWvrBMQDs4UFR2UqhL5yU8xdDcdhTfqUxci";
    let n2_public_key = "n9KWgTyg72yf1AdDoEM3GaDFUPaZNK3uf66uoVpMZeNnGegC9yz2";

    start_container("validator_1", 6001).await;
    start_container("validator_2", 6002).await;

    let ssl_stream1 = connect_to_peer("127.0.0.1", 6001, n2_public_key).await;
    let ssl_stream2 = connect_to_peer("127.0.0.1", 6002, n1_public_key).await;

    handle_conn(ssl_stream1, ssl_stream2).await;
    Ok(())
}
