use crate::packet_client::PacketClient;
use bytes::BytesMut;
use log::error;
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt, ReadHalf, WriteHalf};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio_openssl::SslStream;

const SIZE_KB: usize = 1024;
#[allow(unused)]
const SIZE_MB: usize = 1024 * SIZE_KB;
const SIZE_64KB: usize = 64 * SIZE_KB;
#[allow(unused)]
const SIZE_64MB: usize = 64 * SIZE_MB;

#[derive(Debug)]
pub struct Message {
    pub data: Vec<u8>,
    pub peer_to_port: u16,
}

impl Message {
    pub fn new(data: Vec<u8>, peer_to_port: u16) -> Self {
        Self { data, peer_to_port }
    }
}

#[derive(Debug)]
pub struct Peer {
    pub port: u16,
    pub write_half: WriteHalf<SslStream<TcpStream>>,
    pub read_half: ReadHalf<SslStream<TcpStream>>,
}

impl Peer {
    /// Makes a new peer from a node's perspective.
    /// write_half = the half the interceptor uses to write to that peer
    /// read_half = the half that the node writes to if it wants to send a message to the peer
    pub fn new(
        port: u16,
        write_half: WriteHalf<SslStream<TcpStream>>,
        read_half: ReadHalf<SslStream<TcpStream>>,
    ) -> Self {
        Self {
            port,
            write_half,
            read_half,
        }
    }
}

#[derive(Debug)]
pub struct Node {
    pub port: u16,
    pub peers: Vec<Peer>,
}

impl Node {
    pub fn new(port: u16) -> Self {
        Self {
            port,
            peers: Vec::new(),
        }
    }

    pub fn add_peer(&mut self, peer: Peer) {
        self.peers.push(peer);
    }

    /// This method handles all the messages which this node wants to write to its peers.
    pub fn handle_messages(
        self,
        client: Arc<Mutex<PacketClient>>,
    ) -> (Vec<JoinHandle<()>>, JoinHandle<()>) {
        let (sender, receiver) = mpsc::channel::<Message>();
        let mut read_threads = Vec::new();
        let mut peers = Vec::new();

        for peer in self.peers {
            let read_thread = tokio::spawn(Self::read_loop(
                peer.read_half,
                client.clone(),
                self.port,
                peer.port,
                sender.clone(),
            ));
            read_threads.push(read_thread);
            peers.push((peer.port, peer.write_half));
        }

        let write_thread = tokio::spawn(Self::write_loop(receiver, peers));
        (read_threads, write_thread)
    }

    // This method polls one ReadHalf from the node and spawns a thread that handles the intercepted message.
    async fn read_loop(
        mut read_half: ReadHalf<SslStream<TcpStream>>,
        client: Arc<Mutex<PacketClient>>,
        peer_from_port: u16,
        peer_to_port: u16,
        queue_sender: mpsc::Sender<Message>,
    ) {
        loop {
            let mut buf = BytesMut::with_capacity(SIZE_64KB);
            buf.resize(SIZE_64KB, 0);
            let size_read = read_half
                .read(buf.as_mut())
                .await
                .expect("Could not read from SSL stream");

            let read_moment = Instant::now();

            buf.resize(size_read, 0);
            if size_read == 0 {
                panic!(
                    "SslStream from peer {} to peer {} has been closed.",
                    peer_from_port, peer_to_port
                );
            }

            tokio::spawn(Self::handle_message_and_action(
                buf,
                client.clone(),
                peer_from_port,
                peer_to_port,
                queue_sender.clone(),
                read_moment,
            ));
        }
    }

    /// This method handles an intercepted message.
    /// It asks the controller what action to take, and takes that action.
    /// Once the action has taken, it sends the message to a queue where another thread will
    /// immediately send the message to the corresponding peer.
    async fn handle_message_and_action(
        buffered_message: BytesMut,
        client: Arc<Mutex<PacketClient>>,
        peer_from_port: u16,
        peer_to_port: u16,
        queue: mpsc::Sender<Message>,
        read_moment: Instant,
    ) {
        let message = Self::check_message(buffered_message);
        let response = client
            .lock()
            .await
            .send_packet(message, u32::from(peer_from_port), u32::from(peer_to_port))
            .await
            .expect("Error occurred while requesting message and action from the controller.");

        match response.action {
            0 => (),
            u32::MAX => return,
            delay_ms => {
                let delay_ms = delay_ms as u128;
                let time_elapsed = read_moment.elapsed().as_millis();
                if time_elapsed < delay_ms {
                    let delay_compensated = delay_ms - time_elapsed;
                    tokio::time::sleep(Duration::from_millis(delay_compensated as u64)).await
                }
            }
        }

        queue
            .send(Message::new(response.data, peer_to_port))
            .unwrap_or_else(|_| panic!(
                "Could not write message from {} to {} to the queue.",
                peer_from_port, peer_to_port,
            ));
    }

    /// Checks a message that is contained inside buf if it is valid.
    /// Returns the valid message as a Vec<u8>.
    fn check_message(buf: BytesMut) -> Vec<u8> {
        // Check if the most significant bit turned on, indicating a compressed message
        if (buf[0] & 0b1000_0000) != 0 {
            panic!("Received compressed message: bytes[0] = {:?}", buf[0]);
        }

        // Check if any of the 6 most significant bits are turned on, indicating an unknown header
        if (buf[0] & 0b1111_1100) != 0 {
            panic!("Unknown version header: bytes[0] = {:?}", buf[0]);
        }

        let payload_size = u32::from_be_bytes(buf[0..4].try_into().unwrap()) as usize;

        if buf.len() < 6 + payload_size {
            error!("Message did not fit in the buffer.");
        }

        // return the full message
        buf[0..(6 + payload_size)].to_vec()
    }

    /// This method polls a queue with messages.
    /// It sends every message to the corresponding node immediately.
    async fn write_loop(
        receiver: mpsc::Receiver<Message>,
        mut peers: Vec<(u16, WriteHalf<SslStream<TcpStream>>)>,
    ) {
        loop {
            let message = receiver.recv().unwrap();

            let write_half = &mut peers
                .iter_mut()
                .find(|peer| peer.0 == message.peer_to_port)
                .unwrap()
                .1;

            write_half
                .write_all(&message.data)
                .await
                .expect("Could not write to SSL stream");
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::connection_handler::{Node, SIZE_64KB, SIZE_64MB};
    use bytes::BytesMut;
    use rand::Rng;

    fn create_dummy_payload(len: usize) -> Vec<u8> {
        let mut payload = Vec::new();
        let mut rng = rand::thread_rng();

        for _i in 0..len {
            payload.push(rng.gen::<u8>());
        }

        payload
    }

    fn create_header(first_six_bits: u8, payload_size: usize) -> Vec<u8> {
        if payload_size >= SIZE_64MB {
            panic!("Invalid payload size")
        }

        let first_six_bits = first_six_bits & 0b1111_1100;

        let bytes = payload_size.to_be_bytes();
        match bytes.len() {
            2 => vec![first_six_bits, 0, bytes[0], bytes[1]],
            4 => vec![first_six_bits | bytes[0], bytes[1], bytes[2], bytes[3]],
            8 => vec![first_six_bits | bytes[4], bytes[5], bytes[6], bytes[7]],
            _ => panic!("This should not happen"),
        }
    }

    #[test]
    #[should_panic(expected = "Received compressed message: bytes[0] = 128")]
    fn panic_compressed_message() {
        let mut buf = BytesMut::with_capacity(SIZE_64KB);
        let payload_size: usize = 255;
        buf.extend_from_slice(&create_header(0b1000_0000, payload_size));
        buf.extend_from_slice(&create_dummy_payload(payload_size));
        buf.resize(6 + payload_size, 0);

        let _message = Node::check_message(buf);
    }

    #[test]
    #[should_panic(expected = "Unknown version header: bytes[0] = 40")]
    fn panic_unknown_version_header() {
        let mut buf = BytesMut::with_capacity(SIZE_64KB);
        let payload_size: usize = 44;
        buf.extend_from_slice(&create_header(0b0010_1000, payload_size));
        buf.extend_from_slice(&create_dummy_payload(payload_size));
        buf.resize(6 + payload_size, 0);

        let _message = Node::check_message(buf);
    }

    #[test]
    fn pass_check_message_1() {
        let mut buf = BytesMut::with_capacity(SIZE_64KB);
        let payload_size: usize = 444;
        buf.extend_from_slice(&create_header(0b0000_0000, payload_size));
        buf.extend_from_slice(&create_dummy_payload(payload_size));
        buf.resize(6 + payload_size, 0);

        let message = Node::check_message(buf);

        assert_eq!(message.len(), payload_size + 6)
    }

    #[test]
    fn pass_check_message_2() {
        let mut buf = BytesMut::with_capacity(SIZE_64KB);
        let payload_size: usize = 4444;
        buf.extend_from_slice(&create_header(0b0000_0000, payload_size));
        buf.extend_from_slice(&create_dummy_payload(payload_size));
        buf.resize(6 + payload_size, 0);

        let message = Node::check_message(buf);

        assert_eq!(message.len(), payload_size + 6)
    }
}
