use std::{
  collections::HashMap,
  net::{SocketAddr, UdpSocket},
  sync::mpsc,
  time::Duration,
};

use bincode::Options;
use tracing::{debug, error, info};

use crate::{Command, Msg, Request, Response};

const KEEPALIVE_DURATION: Duration = Duration::from_secs(25);
// TODO: how do I detect if hole punch is closed?
// answer keep alive with either success or another keepalive?
// if there has been no traffic for some time?

fn decode_msg(buf: &[u8]) -> Option<Msg> {
  let config = bincode::DefaultOptions::new().with_fixint_encoding();
  let msg = config.deserialize(buf).unwrap();
  Some(msg)
}

fn server_connect(server_endpoint: SocketAddr, server_forward_port: u16) -> UdpSocket {
  let config = bincode::DefaultOptions::new().with_fixint_encoding();
  let server_socket = UdpSocket::bind("0.0.0.0:0").unwrap();
  server_socket.connect(server_endpoint).unwrap();
  server_socket
    .set_read_timeout(Some(Duration::from_millis(500)))
    .unwrap();
  let msg = Msg::new(Command::Request(Request {
    port: server_forward_port,
    token: 0xdeadbeef,
  }));
  let v = config.serialize(&msg).unwrap();
  server_socket.send(&v).unwrap();

  let mut buf = [0; 2048];
  let (n, _src) = server_socket
    .recv_from(&mut buf)
    .expect("Timed out waiting for response");

  let res: Msg = bincode::deserialize(&buf[0..n]).unwrap();
  match res.command {
    Command::Response(Response::Success) => info!("Server successfully openend port {server_forward_port}"),
    Command::Response(Response::PortInUse) => {
      error!("Requested server port is already in use");
      panic!("");
    }
    cmd => {
      error!("Received invalid response from server {cmd:?}");
      panic!("");
    }
  }
  server_socket.set_read_timeout(None).unwrap();
  server_socket
}

fn spawn_message_forwarder(tx_socket: UdpSocket, msg_channel: mpsc::Receiver<Msg>) {
  let config = bincode::DefaultOptions::new().with_fixint_encoding();
  std::thread::spawn(move || loop {
    let msg = msg_channel.recv().unwrap();
    let bytes = config.serialize(&msg).unwrap();
    tx_socket.send(&bytes).unwrap();
  });
}

fn spawn_keepalive_sender(msg_channel: mpsc::Sender<Msg>) {
  std::thread::spawn(move || loop {
    std::thread::sleep(KEEPALIVE_DURATION);
    debug!("Sending keepalive");
    msg_channel.send(Msg::new(Command::Keepalive)).unwrap();
  });
}

fn receive_msg(rx_socket: &UdpSocket) -> Msg {
  let mut buf = [0; 2048];
  let n = rx_socket.recv(&mut buf).unwrap();
  assert!(n < 2048, "increase buffer size");
  let msg = decode_msg(&buf[0..n]).unwrap();
  msg
}

fn create_client(
  hosts: &mut HashMap<SocketAddr, UdpSocket>,
  client_socket: SocketAddr,
  msg_channel: mpsc::Sender<Msg>,
  forward_to_endpoint: SocketAddr,
) -> &UdpSocket {
  let rx_socket = UdpSocket::bind("0.0.0.0:0").unwrap();
  rx_socket.connect(forward_to_endpoint).unwrap();
  let tx_socket = rx_socket.try_clone().unwrap();

  std::thread::spawn(move || {
    let mut buf = [0; 2048];
    loop {
      let n = rx_socket.recv(&mut buf).unwrap();
      assert!(n < 2048, "increase buffer size");
      msg_channel
        .send(Msg::new(Command::UdpPayload(client_socket, buf[0..n].to_vec())))
        .unwrap();
    }
  });

  hosts.insert(client_socket, tx_socket);
  hosts.get(&client_socket).unwrap()
}

pub fn client(server_endpoint: SocketAddr, server_forward_port: u16, forward_to_endpoint: SocketAddr) {
  let mut hosts = HashMap::<SocketAddr, UdpSocket>::new();
  let server_socket = server_connect(server_endpoint, server_forward_port);

  let (send_msg_channel, recv_msg_channel) = mpsc::channel::<Msg>();
  spawn_message_forwarder(server_socket.try_clone().unwrap(), recv_msg_channel);
  spawn_keepalive_sender(send_msg_channel.clone());

  loop {
    let msg = receive_msg(&server_socket);
    let (src, payload) = match msg.command {
      Command::UdpPayload(src, payload) => (src, payload),
      cmd => {
        error!("Received invalid command from server {cmd:?}");
        continue;
      }
    };

    let socket = if let Some(socket) = hosts.get(&src) {
      socket
    } else {
      create_client(&mut hosts, src, send_msg_channel.clone(), forward_to_endpoint)
    };

    info!("forwarding {payload:?} to {forward_to_endpoint:?} using {socket:?}");
    socket.send_to(&payload, forward_to_endpoint).unwrap();
  }
}
