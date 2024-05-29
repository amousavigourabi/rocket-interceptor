mod docker_manager;
mod packet_client;
mod peer_connector;
use crate::peer_connector::PeerConnector;
use std::{env, fs};
use std::io;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use chrono::Local;
use env_logger::Builder;
use fern::Dispatch;
use log::LevelFilter;
use tokio::sync::Mutex;

fn setup_logger() -> Result<(), fern::InitError> {
    // Get current timestamp
    let now = Local::now();
    let timestamp = now.format("%Y-%m-%d_%H-%M-%S").to_string();

    // Create the logs directory if it doesn't exist
    let logs_dir = format!("./logs/{}", timestamp);
    fs::create_dir_all(&logs_dir)?;

    // Set up Dispatch for env_logger
    let env_logger = Dispatch::new()
        .format(|out, message, record| {
            out.finish(format_args!(
                "{} [{}] {}: {}",
                chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
                record.level(),
                record.target(),
                message
            ))
        })
        .level_for("xrpl_packet_interceptor", LevelFilter::Debug)
        .chain(std::io::stdout());

    // Set up Dispatch for file logging
    let file_logger = Dispatch::new()
        .format(|out, message, record| {
            out.finish(format_args!(
                "{} [{}] {}: {}",
                chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
                record.level(),
                record.target(),
                message
            ))
        })
        .level(LevelFilter::Info)
        .chain(fern::log_file(Path::new(&logs_dir).join("execution.log"))?);

    // Combine the two Dispatches
    let combined_logger = Dispatch::new()
        .chain(env_logger)
        .chain(file_logger);

    // Apply the combined logger
    combined_logger.apply()?;

    Ok(())
}

#[tokio::main]
async fn main() -> io::Result<()> {
    setup_logger().expect("Failed to setup logger");

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

    // Iterate over every unique validator node pair and create a thread for each
    let mut threads = Vec::new();
    for (i, container1) in network.containers.iter().enumerate() {
        for container2 in &network.containers[(i + 1)..network.containers.len()] {
            let (t1, t2) = peer_connector
                .clone()
                .connect_peers(
                    client.clone(),
                    container1.port_peer,
                    container2.port_peer,
                    container1.key_data.validation_public_key.as_str(),
                    container2.key_data.validation_public_key.as_str(),
                )
                .await;

            threads.push(t1);
            threads.push(t2);
        }
    }

    // Wait for all threads to exit (due to error)
    for t in threads {
        t.await.expect("Thread failed");
    }

    network.stop_network().await;

    Ok(())
}
