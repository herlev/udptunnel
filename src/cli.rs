use std::net::SocketAddr;

use clap::{Parser, Subcommand};

use crate::{client, server};

use client::client;
use server::server;

#[derive(Subcommand, Debug, Clone)]
pub enum Commands {
  /// Server
  Server { port: u16 },
  /// Client
  Client {
    server_endpoint: SocketAddr,
    server_forward_port: u16,
    forward_to_endpoint: SocketAddr,
  },
}

// udpt client 127.0.0.1:4321 5000 127.0.0.1:5050
// connects to server on 127.0.0.1 port 4321, and forwards port 5000 from server to 127.0.0.1:5050

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
pub struct Args {
  #[command(subcommand)]
  command: Commands,
}

pub fn run() {
  let args = Args::parse();
  match args.command {
    Commands::Server { port } => server(port),
    Commands::Client {
      server_endpoint,
      server_forward_port,
      forward_to_endpoint,
    } => client(server_endpoint, server_forward_port, forward_to_endpoint),
  }
}
