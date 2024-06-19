#![cfg_attr(all(feature = "nightly-features", test), feature(coverage_attribute))]
mod connection_handler;
mod docker_manager;
mod logger;
mod packet_client;
mod peer_connector;
use crate::connection_handler::{Node, Peer};
use crate::docker_manager::DockerNetwork;
use crate::packet_client::proto::Partition;
use crate::peer_connector::PeerConnector;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::{env, io};
use tokio::sync::Mutex;

fn is_connection_valid(idx_1: u32, idx_2: u32, partitions: &Vec<Partition>) -> bool {
    for p in partitions {
        if p.nodes.contains(&idx_1) && p.nodes.contains(&idx_2) {
            return true;
        }
    }
    false
}

#[tokio::main]
async fn main() -> io::Result<()> {
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();

    ctrlc::set_handler(move || {
        r.store(false, Ordering::SeqCst);
    })
    .expect("Unable to set Ctrl+C handler");

    env::set_var("RUST_LOG", "xrpl_packet_interceptor=info");
    env_logger::init();

    let client = match packet_client::PacketClient::new().await {
        Ok(client) => Arc::new(Mutex::new(client)),
        error => panic!("Error creating client: {:?}", error),
    };

    // Get config from controller
    let network_config = client
        .lock()
        .await
        .get_config()
        .await
        .expect("Could not get config from controller");

    // Init docker network
    let mut network = DockerNetwork::new(network_config.clone());
    network.initialize_network(client.clone()).await;
    network.wait_for_startup().await;

    let peer_connector = PeerConnector::new("127.0.0.1".to_string());

    let mut nodes = Vec::new();
    for node in network.containers.iter() {
        nodes.push(Node::new(node.port_peer as u16));
    }

    let nodes_len = network.containers.len();
    for (i, container1) in network.containers.iter().enumerate() {
        for (j, container2) in network.containers[(i + 1)..nodes_len].iter().enumerate() {
            let j = i + 1 + j;
            if !is_connection_valid(i as u32, j as u32, network_config.partitions.as_ref()) {
                continue;
            }
            let (stream1, stream2) = peer_connector
                .connect_peers(
                    container1.port_peer as u16,
                    container2.port_peer as u16,
                    container1.key_data.validation_public_key.as_str(),
                    container2.key_data.validation_public_key.as_str(),
                )
                .await;
            let (r1, w1) = tokio::io::split(stream1);
            let (r2, w2) = tokio::io::split(stream2);

            let n1 = &mut nodes[i];
            n1.add_peer(Peer::new(container2.port_peer as u16, w2, r1));
            let n2 = &mut nodes[j];
            n2.add_peer(Peer::new(container1.port_peer as u16, w1, r2));
        }
    }

    let mut threads = Vec::new();
    for node in nodes {
        let (mut read_threads, write_thread) = node.handle_messages(client.clone());
        threads.push(write_thread);
        threads.append(&mut read_threads);
    }

    while running.load(Ordering::SeqCst) {}

    for thread in threads {
        thread.abort();
    }

    network.stop_network().await;
    Ok(())
}
