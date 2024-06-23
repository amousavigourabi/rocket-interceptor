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

/// Function that checks whether a connection between two peers should be established or not.
///
/// # Parameters
/// * 'node_1_id' - the ID of the first node.
/// * 'node_2_id' - the ID of the second node.
fn is_valid_connection(node_1_id: u32, node_2_id: u32, partitions: &Vec<Partition>) -> bool {
    for partition in partitions {
        if partition.nodes.contains(&node_1_id) && partition.nodes.contains(&node_2_id) {
            return true;
        }
    }
    false
}

/// The entrypoint for the packet interceptor application.
///
/// This async function first sets up all the Docker containers who run the validator nodes.
/// After that, it establishes connections between all peers as configured.
/// Then, it starts all the threads that handle the messages sent between the peers.
/// Finally, it waits for a Ctrl+C signal to correctly exit.
///
/// # Panics:
/// - If the Ctrl+C handler could not be setup
/// - If the PacketClient could not be setup
/// - If the configuration request failed
#[tokio::main]
async fn main() -> io::Result<()> {
    let running = Arc::new(AtomicBool::new(true));
    let running_cloned = running.clone();

    ctrlc::set_handler(move || {
        running_cloned.store(false, Ordering::SeqCst);
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

    let nodes_length = network.containers.len();
    for (i, container1) in network.containers.iter().enumerate() {
        for (j, container2) in network.containers[(i + 1)..nodes_length].iter().enumerate() {
            let j = i + j + 1; // Adjust 'j' to be the correct index in 'nodes'
            if !is_valid_connection(i as u32, j as u32, network_config.partitions.as_ref()) {
                continue;
            }
            let (connection_half_1, connection_half_2) = peer_connector
                .connect_peers(
                    container1.port_peer as u16,
                    container2.port_peer as u16,
                    container1.key_data.validation_public_key.as_str(),
                    container2.key_data.validation_public_key.as_str(),
                )
                .await;
            let (read_half_1, write_half_1) = tokio::io::split(connection_half_1);
            let (read_half_2, write_half_2) = tokio::io::split(connection_half_2);

            let node_1 = &mut nodes[i];
            node_1.add_peer(Peer::new(
                container2.port_peer as u16,
                write_half_2,
                read_half_1,
            ));
            let node_2 = &mut nodes[j];
            node_2.add_peer(Peer::new(
                container1.port_peer as u16,
                write_half_1,
                read_half_2,
            ));
        }
    }

    let mut message_handlers = Vec::new();
    for node in nodes {
        let (mut read_threads, write_thread) = node.handle_messages(client.clone());
        message_handlers.push(write_thread);
        message_handlers.append(&mut read_threads);
    }

    // Wait for Ctrl+C signal
    while running.load(Ordering::SeqCst) {}

    for message_handler in message_handlers {
        message_handler.abort();
    }

    network.stop_network().await;

    Ok(())
}
