use serde::{Deserialize, Serialize};

mod cli;
mod client;
mod server;

use std::net::SocketAddr;

// TODO: goal, connect two udp nc's to eachother through this tunnel

#[derive(Serialize, Deserialize, Debug)]
struct Request {
  port: u16,
  token: u32,
  // token: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct Msg {
  version: u8,
  command: Command,
}

const VERSION: u8 = 1;

impl Msg {
  pub fn new(command: Command) -> Self {
    Self {
      version: VERSION,
      command,
    }
  }
}

// TODO: Don't send a response if token is invalid
// be stealthy just like wireguard
#[derive(Serialize, Deserialize, Debug)]
enum Response {
  Success,
  PortInUse,
}

#[derive(Serialize, Deserialize, Debug)]
enum Command {
  UdpPayload(SocketAddr, Vec<u8>),
  Request(Request),
  Response(Response),
  Keepalive,
}

fn main() {
  tracing_subscriber::fmt::init();
  cli::run();
}
