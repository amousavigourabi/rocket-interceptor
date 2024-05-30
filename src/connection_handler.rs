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
            let mut buf = BytesMut::with_capacity(64 * 1024);
            buf.resize(64 * 1024, 0);
            let size_read = read_half
                .read(buf.as_mut())
                .await
                .expect("Could not read from SSL stream");

            let read_moment = Instant::now();

            tokio::spawn(Self::handle_message_and_action(
                buf,
                size_read,
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
        mut buf: BytesMut,
        size_read: usize,
        client: Arc<Mutex<PacketClient>>,
        peer_from_port: u16,
        peer_to_port: u16,
        queue: mpsc::Sender<Message>,
        read_moment: Instant,
    ) {
        buf.resize(size_read, 0);
        if size_read == 0 {
            panic!("Current buffer: {}", String::from_utf8_lossy(&buf).trim());
        }
        let bytes = buf.to_vec();

        // Check if the most significant bit turned on, indicating a compressed message
        if bytes[0] & 0x80 != 0 {
            error!("{:?}", bytes[0]);
            panic!("Received compressed message");
        }

        // Check if any of the 6 most significant bits are turned on, indicating an unknown header
        if bytes[0] & 0xFC != 0 {
            error!("{:?}", bytes[0]);
            panic!("Unknown version header")
        }

        let payload_size = u32::from_be_bytes(bytes[0..4].try_into().unwrap()) as usize;

        if payload_size > 64 * 1024 * 1024 {
            panic!("Message size too large");
        }

        if buf.len() < 6 + payload_size {
            error!("Buffer is too short");
            return;
        }

        let message = bytes[0..(6 + payload_size)].to_vec();
        let response = client
            .lock()
            .await
            .send_packet(message, u32::from(peer_from_port), u32::from(peer_to_port))
            .await
            .expect("Error occurred while requesting message from the controller.");

        match response.action {
            0 => (),
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
            .unwrap_or_else(|_| {
                panic!(
                    "Could not write message from {} to {} to the queue.",
                    peer_from_port, peer_to_port
                )
            });
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
