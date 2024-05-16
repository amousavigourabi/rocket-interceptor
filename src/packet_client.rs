use proto::packet_service_client::PacketServiceClient;
use proto::Packet;

pub mod proto {
    tonic::include_proto!("packet");
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // make a client
    let mut client = PacketServiceClient::connect("http://[::1]:50051").await?;

    //Hardcoded for now. The packet data needs to be the intercepted packet data.
    let packet_data: Vec<u8> = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
    let packet_port: u32 = 2;
    //  Hardcoded port

    let request = tonic::Request::new(Packet {
        data: packet_data,
        port: packet_port,
    }); // add port data

    let response = client.send_packet(request).await?; // we send to controller and are waiting for the response
    println!("Response: {:?}", response); // seperate by bytes, port, message

    Ok(())
}
