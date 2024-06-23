use crate::packet_client::PacketClient;
use bytes::BytesMut;
use log::error;
use std::cmp::min;
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

/// Struct that represents an intercepted message.
#[derive(Debug)]
pub struct Message {
    /// The data of the intercepted message.
    pub data: Vec<u8>,
    /// The port of the peer the message is supposed to be sent to.
    pub peer_to_port: u16,
}

impl Message {
    /// Initializes a new Message.
    ///
    /// # Parameters
    /// * 'data' - the data of the intercepted message.
    /// * 'peer_to_port' - the port of the peer the message is supposed to be sent to.
    pub fn new(data: Vec<u8>, peer_to_port: u16) -> Self {
        Self { data, peer_to_port }
    }
}

/// Struct that represents a peer from a node's perspective.
#[derive(Debug)]
pub struct Peer {
    /// The port of the peer where the connection is established to. This is also its ID.
    pub port: u16,
    /// The half that the interceptor uses to write to that peer.
    pub write_half: WriteHalf<SslStream<TcpStream>>,
    /// the half that the node writes to if it wants to send a message to the peer.
    pub read_half: ReadHalf<SslStream<TcpStream>>,
}

impl Peer {
    /// Initializes a new Peer from a node's perspective.
    ///
    /// # Parameters
    /// * 'port' - the port of the peer where the connection is established to. This is also its ID.
    /// * 'write_half' - the half that the interceptor uses to write to that peer.
    /// * 'read_half' - the half that the node writes to if it wants to send a message to the peer.
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

/// Struct that represents a node in the network.
#[derive(Debug)]
pub struct Node {
    /// The port of the peer where connections can be established to. This is also its ID.
    pub port: u16,
    /// The peers the node is connected to.
    pub peers: Vec<Peer>,
}

impl Node {
    /// Initializes a new Node.
    ///
    /// # Parameters
    /// * 'port' - the port of the peer where connections can be established to. This is also its ID.
    pub fn new(port: u16) -> Self {
        Self {
            port,
            peers: Vec::new(),
        }
    }

    /// Adds a Peer to the node's peer list.
    ///
    /// # Parameters
    /// * 'peer' - the Peer to be added to the peer list.
    pub fn add_peer(&mut self, peer: Peer) {
        self.peers.push(peer);
    }

    /// This method handles all the messages which this node wants to write to its peers.
    ///
    /// # Parameters
    /// * 'client' - the PacketClient where it can make requests to the controller for the action of every message.
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

    /// This method reads from one ReadHalf  from the node and spawns a thread that handles the intercepted message.
    /// All of this happens in an infinite loop to handle all the messages.
    ///
    /// # Parameters
    /// * 'read_half' - the ReadHalf where it reads for messages.
    /// * 'client' - the PacketClient used to be passed to 'handle_message_and_action'.
    /// * 'peer_from_port' - the port of the peer where the message came from.
    /// * 'peer_to_port' - the port of the peer the message is sent to.
    /// * 'message_queue_sender' - the queue where the received messages are enqueued
    async fn read_loop(
        mut read_half: ReadHalf<SslStream<TcpStream>>,
        client: Arc<Mutex<PacketClient>>,
        peer_from_port: u16,
        peer_to_port: u16,
        message_queue_sender: mpsc::Sender<Message>,
    ) {
        loop {
            let mut buffer = BytesMut::with_capacity(SIZE_64KB);
            buffer.resize(SIZE_64KB, 0);
            let size_read = read_half
                .read(buffer.as_mut())
                .await
                .expect("Could not read from SSL stream");

            let read_moment = Instant::now();

            buffer.resize(size_read, 0);
            if size_read == 0 {
                panic!(
                    "SslStream from peer {} to peer {} has been closed.",
                    peer_from_port, peer_to_port
                );
            }

            tokio::spawn(Self::handle_message_and_action(
                buffer,
                client.clone(),
                peer_from_port,
                peer_to_port,
                message_queue_sender.clone(),
                read_moment,
            ));
        }
    }

    /// This method handles an intercepted message.
    /// It asks the controller what action to take, and takes that action.
    /// Once the action has taken, it sends the message to a queue where another thread will immediately send the message to the corresponding peer.
    ///
    /// # Parameters
    /// * 'buffered_message' - the received message inside a buffer.
    /// * 'client' - the PacketClient used to send a request to the controller.
    /// * 'peer_from_port' - the port of the peer where the message came from.
    /// * 'peer_to_port' - the port of the peer the message is sent to.
    /// * 'message_queue_sender' - the queue where the received messages are enqueued
    /// * 'read_moment' - the moment the message was read, used if message needs to be delayed.
    ///
    /// # Panics
    /// * If an error occurred while requesting an action from the controller.
    /// * If the message sent to the queue will never be received, meaning there is no receiver.
    async fn handle_message_and_action(
        buffered_message: BytesMut,
        client: Arc<Mutex<PacketClient>>,
        peer_from_port: u16,
        peer_to_port: u16,
        message_queue_sender: mpsc::Sender<Message>,
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
            delay_ms => {
                let delay_ms = min(delay_ms, 30000) as u128;
                let time_elapsed = read_moment.elapsed().as_millis();
                if time_elapsed < delay_ms {
                    let delay_compensated = delay_ms - time_elapsed;
                    tokio::time::sleep(Duration::from_millis(delay_compensated as u64)).await
                }
            }
        }

        message_queue_sender
            .send(Message::new(response.data, peer_to_port))
            .unwrap_or_else(|_| {
                panic!(
                    "Could not write message from {} to {} to the queue.",
                    peer_from_port, peer_to_port,
                )
            });
    }

    /// Checks a message that is contained inside buf if it is valid.
    /// Returns the validated message as a Vec\<u8>\.
    ///
    /// # Parameters
    /// * 'buffered_message' - the message inside a buffer to be checked.
    ///
    /// # Panics
    /// * If a compressed message was received.
    /// * If the message has an unknown version header.
    /// * If the payload size could not be parsed.
    /// * If the payload size is bigger than the buffer's size, meaning it only read a part of the message.
    fn check_message(buffered_message: BytesMut) -> Vec<u8> {
        // Check if the most significant bit turned on, indicating a compressed message
        if (buffered_message[0] & 0b1000_0000) != 0 {
            panic!(
                "Received compressed message: bytes[0] = {:?}",
                buffered_message[0]
            );
        }

        // Check if any of the 6 most significant bits are turned on, indicating an unknown header
        if (buffered_message[0] & 0b1111_1100) != 0 {
            panic!(
                "Unknown version header: bytes[0] = {:?}",
                buffered_message[0]
            );
        }

        let payload_size = u32::from_be_bytes(buffered_message[0..4].try_into().unwrap()) as usize;

        if buffered_message.len() < 6 + payload_size {
            error!("Message did not fit in the buffer.");
        }

        // return the full message
        buffered_message[0..(6 + payload_size)].to_vec()
    }

    /// This method polls a queue with messages.
    /// It sends every message to the corresponding node immediately.
    ///
    /// # Parameters
    /// * 'message_queue_receiver' - the queue where it receives messages to be sent.
    /// * 'peers' - a Vec with port/WriteHalf pairs. Essentially a map from peer ports to their WriteHalves
    ///
    /// # Panics
    /// * If no messages will be ever sent to the queue, meaning all senders have been dropped.
    /// * If the peer's port could not be found in the list of peers.
    /// * If an error occurred while sending the message to the other peer.
    async fn write_loop(
        message_queue_receiver: mpsc::Receiver<Message>,
        mut peers: Vec<(u16, WriteHalf<SslStream<TcpStream>>)>,
    ) {
        loop {
            let message = message_queue_receiver.recv().unwrap();

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
mod unit_tests {
    use crate::connection_handler::{Message, Node, SIZE_64KB, SIZE_64MB};
    use bytes::BytesMut;
    use rand::Rng;

    #[test]
    // #[coverage(off)]  // Only available in nightly build, don't forget to uncomment #![feature(coverage_attribute)] on line 1 of main
    fn test_message_new() {
        let data = vec![1, 2, 3, 4, 5];
        let peer_to_port = 8080;
        let message = Message::new(data.clone(), peer_to_port);

        assert_eq!(message.data, data);
        assert_eq!(message.peer_to_port, peer_to_port);
    }

    #[test]
    // #[coverage(off)]  // Only available in nightly build, don't forget to uncomment #![feature(coverage_attribute)] on line 1 of main
    fn test_node_new() {
        let port = 8080;
        let node = Node::new(port);

        assert_eq!(node.port, port);
        assert_eq!(node.peers.len(), 0);
    }

    fn create_dummy_payload(length: usize) -> Vec<u8> {
        let mut payload = Vec::new();
        let mut rng = rand::thread_rng();

        for _i in 0..length {
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
    // #[coverage(off)]  // Only available in nightly build, don't forget to uncomment #![feature(coverage_attribute)] on line 1 of main
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
    // #[coverage(off)]  // Only available in nightly build, don't forget to uncomment #![feature(coverage_attribute)] on line 1 of main
    #[should_panic(expected = "Unknown version header: bytes[0] = 40")]
    fn panic_unknown_version_header() {
        let mut buffer = BytesMut::with_capacity(SIZE_64KB);
        let payload_size: usize = 44;
        buffer.extend_from_slice(&create_header(0b0010_1000, payload_size));
        buffer.extend_from_slice(&create_dummy_payload(payload_size));
        buffer.resize(6 + payload_size, 0);

        let _message = Node::check_message(buffer);
    }

    #[test]
    // #[coverage(off)]  // Only available in nightly build, don't forget to uncomment #![feature(coverage_attribute)] on line 1 of main
    fn pass_check_message_1() {
        let mut buffer = BytesMut::with_capacity(SIZE_64KB);
        let payload_size: usize = 444;
        buffer.extend_from_slice(&create_header(0b0000_0000, payload_size));
        buffer.extend_from_slice(&create_dummy_payload(payload_size));
        buffer.resize(6 + payload_size, 0);

        let message = Node::check_message(buffer);

        assert_eq!(message.len(), payload_size + 6)
    }

    #[test]
    // #[coverage(off)]  // Only available in nightly build, don't forget to uncomment #![feature(coverage_attribute)] on line 1 of main
    fn pass_check_message_2() {
        let mut buffer = BytesMut::with_capacity(SIZE_64KB);
        let payload_size: usize = 4444;
        buffer.extend_from_slice(&create_header(0b0000_0000, payload_size));
        buffer.extend_from_slice(&create_dummy_payload(payload_size));
        buffer.resize(6 + payload_size, 0);

        let message = Node::check_message(buffer);

        assert_eq!(message.len(), payload_size + 6)
    }
}
