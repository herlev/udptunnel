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

pub fn client(server_endpoint: SocketAddr, server_forward_port: u16, forward_to_endpoint: SocketAddr) {
  let mut hosts = HashMap::<SocketAddr, UdpSocket>::new();
  let config = bincode::DefaultOptions::new().with_fixint_encoding();
  let server_socket = UdpSocket::bind("0.0.0.0:0").unwrap(); // automatically get port from OS
  debug!("using {server_socket:?}");

  debug!("using {server_endpoint:?}");
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
      return;
    }
    cmd => {
      error!("Received invalid response from server {cmd:?}");
      return;
    }
  }

  let tx_socket = server_socket.try_clone().unwrap();
  let (s, r) = mpsc::channel::<Msg>();
  std::thread::spawn(move || loop {
    let msg = r.recv().unwrap();
    let bytes = config.serialize(&msg).unwrap();
    tx_socket.send(&bytes).unwrap();
  });

  let keepalive_sender = s.clone();
  std::thread::spawn(move || loop {
    std::thread::sleep(KEEPALIVE_DURATION);
    debug!("Sending keepalive");
    keepalive_sender.send(Msg::new(Command::Keepalive)).unwrap();
  });

  let mut buf = [0; 2048];
  server_socket.set_read_timeout(None).unwrap();
  loop {
    let n = server_socket.recv(&mut buf).unwrap();
    assert!(n < 2048, "increase buffer size");
    let msg = decode_msg(&buf[0..n]).unwrap();

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
      let rx_socket = UdpSocket::bind("127.0.0.1:0").unwrap();
      // rx_socket.connect(forward_to_endpoint).unwrap();
      let tx_socket = rx_socket.try_clone().unwrap();
      let s = s.clone();
      std::thread::spawn(move || {
        let mut buf = [0; 2048];
        loop {
          let (n, src2) = rx_socket.recv_from(&mut buf).unwrap();
          assert!(src2 == forward_to_endpoint);
          assert!(n < 2048, "increase buffer size");
          s.send(Msg::new(Command::UdpPayload(src, buf[0..n].to_vec()))).unwrap();
        }
      });
      hosts.insert(src, tx_socket);
      hosts.get(&src).unwrap()
    };

    // let socket = hosts
    //   .entry(src)
    //   .or_insert_with(|| UdpSocket::bind("127.0.0.1:0").unwrap());

    info!("forwarding {payload:?} to {forward_to_endpoint:?} using {socket:?}");
    socket.send_to(&payload, forward_to_endpoint).unwrap();
  }
}
