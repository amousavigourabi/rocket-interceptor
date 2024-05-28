mod connection_handler;
mod docker_manager;
mod packet_client;
mod peer_connector;
use crate::connection_handler::{Node, Peer};
use crate::peer_connector::PeerConnector;
use std::env;
use std::io;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

#[tokio::main]
async fn main() -> io::Result<()> {
    env::set_var("RUST_LOG", "xrpl_packet_interceptor=DEBUG");
    env_logger::init();

    let client = match packet_client::PacketClient::new().await {
        Ok(client) => Arc::new(Mutex::new(client)),
        error => panic!("Error creating client: {:?}", error),
    };

    // Init docker network
    let network_config = docker_manager::get_config();
    let mut network = docker_manager::DockerNetwork::new(network_config);
    network.initialize_network(client.clone()).await;

    tokio::time::sleep(Duration::from_secs(3)).await;

    let peer_connector = PeerConnector::new("127.0.0.1".to_string());

    let mut nodes = Vec::new();
    for node in network.containers.iter() {
        nodes.push(Node::new(node.port_peer));
    }

    let nodes_len = network.containers.len();
    for (i, container1) in network.containers.iter().enumerate() {
        for (j, container2) in network.containers[(i + 1)..nodes_len].iter().enumerate() {
            let j = i + 1 + j;
            let (stream1, stream2) = peer_connector
                .connect_peers(
                    container1.port_peer,
                    container2.port_peer,
                    container1.key_data.validation_public_key.as_str(),
                    container2.key_data.validation_public_key.as_str(),
                )
                .await;
            let (r1, w1) = tokio::io::split(stream1);
            let (r2, w2) = tokio::io::split(stream2);

            let n1 = &mut nodes[i];
            n1.add_peer(Peer::new(container2.port_peer, w2, r1));
            let n2 = &mut nodes[j];
            n2.add_peer(Peer::new(container1.port_peer, w1, r2));
        }
    }

    let mut threads = Vec::new();
    for node in nodes {
        let (mut read_threads, write_thread) = node.handle_messages(client.clone());
        threads.push(write_thread);
        threads.append(&mut read_threads);
    }

    for thread in threads {
        thread.await.expect("thread failed");
    }

    network.stop_network().await;

    Ok(())
}
