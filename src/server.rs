use crate::{Command, Msg, Request, Response, VERSION};
use bincode::Options;
use std::{
  collections::HashMap,
  net::{SocketAddr, UdpSocket},
  sync::mpsc,
  time::Instant,
};
use tracing::{error, info, warn};

type Client = SocketAddr;
type MainWriteChannel = mpsc::Sender<(Client, Msg)>;
type MainReadChannel = mpsc::Receiver<(Client, Msg)>;
type Clients = HashMap<SocketAddr, ClientInfo>;

// TODO: fix duplication of config

fn token_valid(_token: u32) -> bool {
  true
}

struct ClientInfo {
  _forwarding_port: u16,
  last_used: Instant,
  tx_channel: mpsc::Sender<(SocketAddr, Vec<u8>)>,
}

fn handle_request_command(
  clients: &mut Clients,
  client: SocketAddr,
  req: Request,
  main_w_tx_channel: &MainWriteChannel,
) {
  assert!(!clients.contains_key(&client));
  // let config = bincode::DefaultOptions::new().with_fixint_encoding();
  if !token_valid(req.token) {
    warn!("invalid token: {}", req.token);
    return;
  }
  let rx_socket = match UdpSocket::bind(("0.0.0.0", req.port)) {
    Ok(s) => s,
    Err(e) => {
      error!("{e}");
      main_w_tx_channel
        .send((client, Msg::new(Command::Response(Response::PortInUse))))
        .unwrap();
      return;
    }
  };

  let tx_socket = rx_socket.try_clone().unwrap();
  main_w_tx_channel
    .send((client, Msg::new(Command::Response(Response::Success))))
    .unwrap();

  let c = main_w_tx_channel.clone();
  let (s, r) = mpsc::channel();
  // TODO: do I need to keep a handle on t1 and t2 or can I kill these by dropping the channel?
  let _t1 = std::thread::spawn(move || {
    port_r(client, rx_socket, c);
  });
  let _t2 = std::thread::spawn(move || {
    port_w(tx_socket, r);
  });
  let client_info = ClientInfo {
    _forwarding_port: req.port,
    last_used: Instant::now(),
    tx_channel: s,
  };
  clients.insert(client, client_info);
}

fn handle_msg(clients: &mut Clients, client: SocketAddr, msg: Msg, main_w_tx_channel: &MainWriteChannel) {
  // let config = bincode::DefaultOptions::new().with_fixint_encoding();
  info!("got msg: {msg:?}");
  assert!(msg.version == VERSION);
  match msg.command {
    crate::Command::UdpPayload(src, payload) => {
      match clients.get_mut(&client) {
        Some(client_info) => {
          client_info.tx_channel.send((src, payload)).unwrap();
          client_info.last_used = Instant::now();
        }
        None => error!("received udp payload from unauthenticated client {client:?}"),
      };
    }
    crate::Command::Request(req) => handle_request_command(clients, client, req, main_w_tx_channel),
    crate::Command::Keepalive => match clients.get_mut(&client) {
      Some(client_info) => {
        client_info.last_used = Instant::now();
      }
      None => error!("received keepalive from unauthenticated client {client:?}"),
    },
    crate::Command::Response(_) => error!("received response from client"),
  };
}

fn port_w(tx_socket: UdpSocket, chan: mpsc::Receiver<(SocketAddr, Vec<u8>)>) {
  // TODO: add match and warn!
  loop {
    let (host, data) = chan.recv().unwrap();
    tx_socket.send_to(&data, host).unwrap();
  }
}

fn port_r(client: Client, rx_socket: UdpSocket, main_w_tx_channel: MainWriteChannel) {
  // TODO: add some way to kill this
  // maybe just wrap a mpsc in a lil class and do a try_recv
  // For terminating threads, channels are way more expensive than a simple Arc<AtomicBool>. You can encapsulate it in nice API too:
  info!("port_r thread started for {client:?}");
  let mut buf = [0; 2048];
  loop {
    let (n, src) = rx_socket.recv_from(&mut buf).unwrap();
    assert!(n < 2048, "increase buffer size");
    info!("{rx_socket:?} received {:?}", &buf[0..n]);
    let msg = Msg::new(Command::UdpPayload(src, buf[0..n].to_vec()));
    main_w_tx_channel.send((client, msg)).unwrap();
  }
}

/// Sends data to clients, that it receives on a channel
fn main_w(tx_socket: UdpSocket, rx_channel: MainReadChannel) {
  let config = bincode::DefaultOptions::new().with_fixint_encoding();
  loop {
    let (client, msg) = rx_channel.recv().unwrap();
    let bytes = config.serialize(&msg).unwrap();
    info!("sending {msg:?} to {client:?}");
    tx_socket.send_to(&bytes, client).unwrap();
  }
}

fn main_r(rx_socket: UdpSocket, main_w_tx_channel: MainWriteChannel) {
  let mut clients = Clients::new();
  let config = bincode::DefaultOptions::new().with_fixint_encoding();
  let mut buf = [0; 2048];
  loop {
    let (n, src) = rx_socket.recv_from(&mut buf).unwrap();
    assert!(n < 2048, "increase buffer size");
    info!("Got new connection from {}:{}", src.ip(), src.port());
    let decoded: bincode::Result<Msg> = config.deserialize(&buf[0..n]);
    match decoded {
      Err(_e) => warn!("got invalid data from client"),
      Ok(msg) => handle_msg(&mut clients, src, msg, &main_w_tx_channel),
    }
  }
}

pub fn server(port: u16) {
  let tx_socket = UdpSocket::bind(("0.0.0.0", port)).unwrap();
  info!("listening on {tx_socket:?}");
  let rx_socket = tx_socket.try_clone().unwrap();
  let (sender, receiver) = mpsc::channel();
  let t1 = std::thread::spawn(move || main_r(rx_socket, sender));
  let t2 = std::thread::spawn(move || main_w(tx_socket, receiver));

  t1.join().unwrap();
  t2.join().unwrap();
}

// we don't need to transmit the port to the client, since each client is only associated with one port

// the main port (4321) has two associated threads main_r and main_w
// main_r reads from the udp socket and writes to multiple channels. One channel to main_w and one for each client
// main_w reads from a channel and writes to the udp socket

// port_r reads from a udp port and writes to the channel of main_w
// port_w reads from its own channel and writes to the udp socket

// main_w channel: Channel<(Port, SocketAddr, UdpPayload)>
// port_w channel: Channel<(SocketAddr, UdpPayload)>
