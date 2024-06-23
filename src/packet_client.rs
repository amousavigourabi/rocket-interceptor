//! This module is responsible for making and handling requests to the controller.

use crate::log;
use crate::logger::EXECUTION_LOG;
use crate::packet_client::proto::{Config, GetConfig, PacketAck};
use log::info;
use proto::packet_service_client::PacketServiceClient;
use proto::{Packet, ValidatorNodeInfo};

pub mod proto {
    tonic::include_proto!("packet");
}

/// Struct that represents the object that is able to call the controller module.
#[derive(Debug)]
pub struct PacketClient {
    pub client: PacketServiceClient<tonic::transport::Channel>,
}

impl PacketClient {
    /// Initializes a new PacketClient that connects to the controller.
    pub async fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let client = PacketServiceClient::connect("http://[::1]:50051").await?;
        Ok(Self { client })
    }

    /// Sends an intercepted message to the controller, asking for an action.
    ///
    /// # Parameters
    /// * 'packet_data' - the data of the intercepted message.
    /// * 'packet_from_port' - the port of the node where the message came from.
    /// * 'packet_to_port' - the port of the node where the message is sent to.
    pub async fn send_packet(
        &mut self,
        packet_data: Vec<u8>,
        packet_from_port: u32,
        packet_to_port: u32,
    ) -> Result<PacketAck, Box<dyn std::error::Error>> {
        if packet_data.is_empty() {
            return Err("Packet data is empty".into());
        }

        match packet_from_port {
            u32::MAX => return Err("packet_from_port not set properly".into()),
            port => port,
        };

        match packet_to_port {
            u32::MAX => return Err("packet_to_port not set properly".into()),
            port => port,
        };

        let packet = Packet {
            data: packet_data.clone(),
            from_port: packet_from_port,
            to_port: packet_to_port,
        };

        let request = tonic::Request::new(packet);

        let response = self.client.send_packet(request).await?.into_inner();
        log!(
            EXECUTION_LOG,
            "{},{},{},{},{}",
            hex::encode(packet_data),
            packet_from_port,
            packet_to_port,
            hex::encode(&response.data),
            response.action
        );

        Ok(response)
    }

    /// Sends the info of all ValidatorNodes to the controller.
    ///
    /// # Parameters
    /// * 'validator_node_info_list' - A list of all the info of the ValidatorNodes.
    pub async fn send_validator_node_info(
        &mut self,
        validator_node_info_list: Vec<ValidatorNodeInfo>,
    ) -> Result<String, Box<dyn std::error::Error>> {
        let request = tonic::Request::new(tokio_stream::iter(validator_node_info_list.into_iter()));
        let response = self
            .client
            .send_validator_node_info(request)
            .await?
            .into_inner();
        info!("Response: {:?}", response);

        Ok(response.status)
    }

    /// Sends a request to the controller asking for the network configuration.
    pub async fn get_config(&mut self) -> Result<Config, Box<dyn std::error::Error>> {
        let request = tonic::Request::new(GetConfig {});
        let response = self.client.get_config(request).await?.into_inner();
        info!("Response: {:?}", response);

        Ok(response)
    }
}

// For these test to work, the controller needs to be running.
#[cfg(test)]
mod integration_tests {
    use super::*;
    async fn setup() -> PacketClient {
        PacketClient::new().await.unwrap()
    }

    #[tokio::test]
    async fn assert_result() {
        let mut client = setup().await;
        let packet_data: Vec<u8> = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10];

        let result = client.send_packet(packet_data, 2, 3).await;

        assert!(result.is_ok());
    }
    #[tokio::test]
    async fn assert_empty_bytes() {
        let mut client = setup().await;
        let packet_data: Vec<u8> = vec![];

        let result = client.send_packet(packet_data, 2, 3).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn assert_empty_port() {
        let mut client = setup().await;
        let packet_data: Vec<u8> = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
        let packet_from_port: u32 = u32::MAX;

        let result = client.send_packet(packet_data, packet_from_port, 3).await;

        assert!(result.is_err());
    }
}
