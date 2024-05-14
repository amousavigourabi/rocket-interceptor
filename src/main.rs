mod peer_connector;

use std::io;
use log::*;
use std::env::current_dir;
use std::env::set_var;
use std::process::{Command, Stdio};
use std::time::Duration;
use tokio::time::sleep;
use crate::peer_connector::PeerConnector;

async fn start_container(name: &str, port: u16) {
    debug!("Starting docker container: {}", name);
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
        .expect("Failed to start docker container");

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

    let peer_connector = PeerConnector { ip_addr: "127.0.0.1", base_port: 6000 };
    let (t1, t2) = peer_connector.connect_peers(1, 2, &n1_public_key, &n2_public_key).await;

    // Await the threads for now (never stopping, unless error occurs),
    // we need to add a concrete stopping condition later on
    let _ = t1.await;
    let _ = t2.await;

    Ok(())
}
