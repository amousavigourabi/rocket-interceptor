use std::future::Future;
use std::io::Write;
use std::ptr::read;
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};
use bytes::BytesMut;
use log::error;
use tokio::io::{AsyncReadExt, AsyncWriteExt, Join, ReadHalf, WriteHalf};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio_openssl::SslStream;
use crate::packet_client::PacketClient;

pub struct Message {
    pub data: Vec<u8>,
    pub port_to: u16,
}

impl Message {
    pub fn new(data: Vec<u8>, port_to: u16) -> Self {
        Self { data, port_to }
    }
}

pub struct Node {
    pub port: u16,
    pub peers: Vec<Peer>,
}

pub struct Peer {
    pub port: u16,
    pub write_half: WriteHalf<SslStream<TcpStream>>,
    pub read_half: ReadHalf<SslStream<TcpStream>>,
}

impl Node {
    pub fn new(port: u16) -> Self {
        Self { port, peers: Vec::new() }
    }

    pub fn add_peer(&mut self, peer: Peer) {
        self.peers.push(peer);
    }

    pub fn start(mut self, client: Arc<Mutex<PacketClient>>) -> (Vec<JoinHandle<()>>, JoinHandle<()>) {
        let (sender, receiver) = mpsc::channel::<Message>();
        let mut read_threads = Vec::new();
        let mut peer_ports = Vec::new();
        let mut peer_write_halves = Vec::new();

        for peer in self.peers {
            let read_thread = tokio::spawn(
                Self::handle_read(
                    peer.read_half,
                    client.clone(),
                    self.port,
                    peer.port,
                    sender.clone(),
                )
            );
            read_threads.push(read_thread);
            peer_ports.push(peer.port);
            peer_write_halves.push(peer.write_half);
        };

        let write_thread = tokio::spawn(Self::handle_write(receiver, peer_ports, peer_write_halves));
        (read_threads, write_thread)
    }

    async fn handle_read(
        mut read_half: ReadHalf<SslStream<TcpStream>>,
        client: Arc<Mutex<PacketClient>>,
        peer_from_port: u16,
        peer_to_port: u16,
        queue: mpsc::Sender<Message>
    ) {
        loop {
            let mut buf = BytesMut::with_capacity(64 * 1024);
            buf.resize(64 * 1024, 0);
            let size = read_half
                .read(buf.as_mut())
                .await
                .expect("Could not read from SSL stream");

            let read_moment = Instant::now();

            buf.resize(size, 0);
            if size == 0 {
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

            tokio::spawn(Self::handle_action(
               client.clone(),
               peer_from_port,
               peer_to_port,
               queue.clone(),
               read_moment,
               message,
            ));
        }
    }

    async fn handle_action(
        client: Arc<Mutex<PacketClient>>,
        peer_from_port: u16,
        peer_to_port: u16,
        queue: mpsc::Sender<Message>,
        read_moment: Instant,
        message: Vec<u8>,
    ) {
        let response = client
            .lock()
            .await
            .send_packet(message, u32::from(peer_from_port), u32::from(peer_to_port))
            .await
            .unwrap();

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

        queue.send(Message::new(response.data, peer_to_port)).unwrap()
    }

    async fn handle_write(
        receiver: mpsc::Receiver<Message>,
        peer_ports: Vec<u16>,
        mut peer_write_halves: Vec<WriteHalf<SslStream<TcpStream>>>,
    ) {
        loop {
            let message = receiver.recv().unwrap();
            let peer_index = *peer_ports.iter().find(|&&peer| peer == message.port_to).unwrap();

            let mut write_half = &mut peer_write_halves[peer_index as usize];

            write_half
                .write_all(&message.data)
                .await
                .expect("Could not write to SSL stream");
        }
    }
}


impl Peer {
    pub fn new(
        port: u16,
        write_half: WriteHalf<SslStream<TcpStream>>,
        read_half: ReadHalf<SslStream<TcpStream>>,
    ) -> Self {
        Self { port, write_half, read_half }
    }
}