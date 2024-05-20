use proto::packet_service_client::PacketServiceClient;
use proto::Packet;

pub mod proto {
    tonic::include_proto!("packet");
}

pub struct PacketClient {
    pub client: PacketServiceClient<tonic::transport::Channel>,
}

impl PacketClient {
    pub async fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let client = PacketServiceClient::connect("http://[::1]:50051").await?;
        Ok(Self { client })
    }

    pub async fn send_packet(
        &mut self,
        packet_data: Vec<u8>,
        packet_from_port: u32,
        packet_to_port: u32,
    ) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        if packet_data.is_empty() {
            return Err("Packet data is empty".into());
        }

        match packet_from_port {
            u32::MAX => return Err("Port not set properly".into()),
            port => port,
        };

        match packet_to_port {
            u32::MAX => return Err("Port not set properly".into()),
            port => port,
        };

        let request = tonic::Request::new(Packet {
            data: packet_data,
            from_port: packet_from_port,
            to_port: packet_to_port,
        });

        let response = self.client.send_packet(request).await?.into_inner(); // we send to controller and are waiting for the response
        println!("Response: {:?}", response);

        Ok(response.data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    async fn setup() -> PacketClient {
        PacketClient::new().await.unwrap()
    }

    #[tokio::test]
    async fn assert_result() {
        let mut client = setup().await;
        let packet_data: Vec<u8> = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10];

        // Call the async function and obtain the result
        let result = client.send_packet(packet_data, 2, 3).await;

        // Assert that the result is Ok
        assert!(result.is_ok());
    }
    #[tokio::test]
    async fn assert_empty_bytes() {
        let mut client = setup().await;
        // Prepare a request with invalid data
        let packet_data: Vec<u8> = vec![]; // Empty data

        // Call the async function and obtain the result
        let result = client.send_packet(packet_data, 2, 3).await;

        // Assert that the result is not Ok (i.e., Err)
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn assert_empty_port() {
        let mut client = setup().await;
        // Prepare a request with invalid data
        let packet_data: Vec<u8> = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10]; // Empty data
        let packet_from_port: u32 = u32::MAX;

        // Call the async function and obtain the result
        let result = client.send_packet(packet_data, packet_from_port, 3).await;

        // Assert that the result is not Ok (i.e., Err)
        assert!(result.is_err());
    }
}
