use proto::packet_service_client::PacketServiceClient;
use proto::Packet;


pub mod proto {
    tonic::include_proto!("packet");
}

async fn send_packet(request: tonic::Request<Packet>) -> Result<(), Box<dyn std::error::Error>> {
    let mut client = PacketServiceClient::connect("http://[::1]:50051").await?;

    let response = client.send_packet(request).await?; // we send to controller and are waiting for the response
    println!("Response: {:?}", response); // seperate by bytes, port, message

    Ok(())
}

#[tokio::main]
async fn main() {
    let packet_data: Vec<u8> = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
    let packet_port: u32 = 2;
    let request = tonic::Request::new(Packet {
        data: packet_data,
        port: packet_port,
    });
    if let Err(err) = send_packet(request).await {
        eprintln!("Error: {}", err);
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn assert_result() {
        // Call the async function
        let packet_data: Vec<u8> = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
        let packet_port: u32 = 2;
        let request = tonic::Request::new(Packet {
            data: packet_data,
            port: packet_port,
        });
        let result = send_packet(request);

        // Assert that the result is Ok
        assert!(result.await.is_ok());
    }
}
