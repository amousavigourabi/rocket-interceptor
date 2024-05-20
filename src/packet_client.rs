use proto::packet_service_client::PacketServiceClient;
use proto::Packet;

pub mod proto {
    tonic::include_proto!("packet");
}

pub(crate) async fn send_packet(packet_data: Vec<u8>, packet_port: u32) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    if packet_data.is_empty() {
        return Err("Packet data is empty".into());
    }

    match packet_port {
        u32::MAX => return Err("Port not set properly".into()),
        port => port,
    };

    let request = tonic::Request::new(Packet {
        data: packet_data,
        port: packet_port,
    });

    let mut client = PacketServiceClient::connect("http://[::1]:50051").await?;

    let response = client.send_packet(request).await?.into_inner(); // we send to controller and are waiting for the response
    println!("Response: {:?}", response);
    // seperate by bytes, port, message

    Ok(response.data)
}

#[tokio::main]
async fn main() {
    let packet_data: Vec<u8> = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
    let packet_port: u32 = 2;
    if let Err(err) = send_packet(packet_data, packet_port).await {
        eprintln!("Error: {}", err);
        std::process::exit(1);
    }
}

// #[cfg(test)]
// mod tests {
//     use super::*;
//
//     #[tokio::test]
//     async fn assert_result() {
//         // Call the async function
//         let packet_data: Vec<u8> = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
//         let packet_port: u32 = 2;
//         let request = tonic::Request::new(Packet {
//             data: packet_data,
//             port: packet_port,
//         });
//         let result = send_packet(request);
//
//         // Assert that the result is Ok
//         assert!(result.await.is_ok());
//     }
//     #[tokio::test]
//     async fn assert_empty_bytes() {
//         // Prepare a request with invalid data
//         let packet_data: Vec<u8> = vec![]; // Empty data
//         let packet_port: u32 = 2;
//         let request = tonic::Request::new(Packet {
//             data: packet_data,
//             port: packet_port,
//         });
//
//         // Call the async function and obtain the result
//         let result = send_packet(request).await;
//
//         // Assert that the result is not Ok (i.e., Err)
//         assert!(result.is_err());
//     }
//
//     #[tokio::test]
//     async fn assert_empty_port() {
//         // Prepare a request with invalid data
//         let packet_data: Vec<u8> = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10]; // Empty data
//         let packet_port: u32 = u32::MAX;
//         let request = tonic::Request::new(Packet {
//             data: packet_data,
//             port: packet_port,
//         });
//
//         // Call the async function and obtain the result
//         let result = send_packet(request).await;
//
//         // Assert that the result is not Ok (i.e., Err)
//         assert!(result.is_err());
//     }
// }
