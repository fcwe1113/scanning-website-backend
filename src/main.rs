mod connection_info;
mod token_exchange;
mod screen_state;
mod client_connection;

use std::{
    string::String,
    env,
    iter,
    net::SocketAddr,
    sync::{Arc, Mutex}
};
use crate::connection_info::ConnectionInfo;
use crate::token_exchange::*;
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::{accept_async, tungstenite::protocol::Message};
use futures::{StreamExt, SinkExt};
use clap::builder::Str;
use log::{info, error, debug};
use rand::{rng, Rng};
use tracing_subscriber::{util::SubscriberInitExt, prelude::__tracing_subscriber_SubscriberExt};
use tungstenite::Utf8Bytes;
use unicode_segmentation::UnicodeSegmentation;
use rand::{SeedableRng, RngCore};
use rand::distr::Alphanumeric;
use rand_chacha::ChaCha20Rng;
use crate::client_connection::client_connection;
use crate::screen_state::ScreenState;

#[tokio::main]
async fn main() {

    let mut rng = ChaCha20Rng::from_os_rng();

    for i in 0..10 {
        // salt generator for account creation
        println!("{}", (0..20).map(|_| char::from(rng.random_range(32..127))).collect::<String>());
    }

    // Initialize the logger
    tracing_subscriber::registry()
        .with(
            // for some reason to choose types of logs to allow past the filter cannot have spacebars so "info, debug" would not work
            // GREEEEEEAAAAAAATTTTTT
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "error,info,debug".into()).add_directive("mycrate".parse().unwrap()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // make the master list of all current active connections
    let mut connections_list: Vec<ConnectionInfo> = Vec::new();
    // slap a lock on that guy bc guy is popular and getting harassed by multiple ppl at once is bad
    let connections_list_lock = Arc::new(Mutex::new(connections_list));

    // Get the address to bind to aka which address the server listens to
    let addr = env::args().nth(1).unwrap_or_else(|| "127.0.0.1:8080".to_string());
    let addr: SocketAddr = addr.parse().expect("Invalid address");

    // Create the TCP listener
    let listener = TcpListener::bind(&addr).await.expect("Failed to bind");

    info!("Listening on: {}", addr);

    // waits for an incoming connection and runs the loop if there is one coming in
    while let Ok((stream, _)) = listener.accept().await {
        let addr = stream.peer_addr().unwrap();
        let mut is_duplicate = false;

        // for some reason react intentionally opens the same websocket twice with one new websocket object init
        // we loop thru the connections list and if the ip is already on the list we drop the request
        // AAAANNNNDDDD it doesnt work as react connects in with different port for each connection
        // AAAANNNNDDDD it just uses the first connection and ignores the second connection memory leak style
        // todo0
        // either disable this dickish behaviour in react or somehow find a way to disconnect the redundant connection here
        // disabled it on react by disabling strict mode yaaaaaaaaaaaaaaayyyyyy

        {
            // this code here is surrounded with {} bc we want to ensure the entire code block here locks up
            // the list and prevent anything else from interfering and escapes the duplicate check
            // effectively this ensures one connection gets established before the next new client can start the
            // connection process
            let mut temp_connections_list = &mut connections_list_lock.lock().unwrap();
            for connections in temp_connections_list.iter() {
                if connections.addr == addr {
                    is_duplicate = true;
                    break;
                }
            }

            if !is_duplicate {
                temp_connections_list.push(ConnectionInfo::new(addr, String::from("-1")));
                info!("New connection from: {}", addr);
                debug!("{:#?}", temp_connections_list);
                // Spawn a new task for each connection
                // note this line makes a new thread for each connection
                // 0a generate valid token
                tokio::spawn(client_connection(stream, addr, token_gen(&*temp_connections_list), false, connections_list_lock.clone()));
            } else {
                info!("duplicate connection request from: {}, dropping", addr);
            }
        }
    }
}



