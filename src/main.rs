mod connection_info;

use std::{
    string::String,
    env,
    iter,
    net::SocketAddr,
    sync::{Arc, Mutex}
};
use crate::connection_info::ConnectionInfo;
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::{accept_async, tungstenite::protocol::Message};
use futures::{StreamExt, SinkExt};
use clap::builder::Str;
use log::{info, error, debug};
use rand::Rng;
use tracing_subscriber::{util::SubscriberInitExt, prelude::__tracing_subscriber_SubscriberExt};
use tungstenite::Utf8Bytes;
use unicode_segmentation::UnicodeSegmentation;


#[tokio::main]
async fn main() {
    // text truncating
    // either sanitise input into only sending ascii
    // or do the jank below
    // let mut test = String::from("你tester");
    // let mut slice = test.unicode_words().collect::<Vec<&str>>();
    // println!("{:?}", slice);


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
        // todo
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
                let token = token_gen(&temp_connections_list);
                temp_connections_list.push(ConnectionInfo::new(addr, token.clone()));
                info!("New connection from: {}", addr);
                debug!("{:#?}", temp_connections_list);
                // Spawn a new task for each connection
                // note this line makes a new thread for each connection
                tokio::spawn(handle_connection(stream, addr, token, connections_list_lock.clone()));
            } else {
                info!("duplicate connection request from: {}, dropping", addr);
            }
        }
    }
}

async fn handle_connection(stream: TcpStream, addr: SocketAddr, token: String, list_lock: Arc<Mutex<Vec<ConnectionInfo>>>) {
    // note we dont want to lock the list and pass the list in by ref
    // do that and only one client can access the list until it dcs

    // this function handles the connection coming in
    // it first upgrades the connection from a normal request to a tcp channel
    // then it inits the receiver and sender so the server can talk to the client both ways

    // Accept the WebSocket connection
    let ws_stream = match accept_async(stream).await {
        Ok(ws) => {
            info!("New WebSocket connection made with {}", addr);
            ws
        },
        Err(e) => {
            error!("Error during the websocket handshake for address {}: {}", addr, e);
            return;
        }
    };

    // Split the WebSocket stream into a sender and receiver
    let (mut sender, mut receiver) = ws_stream.split();

    // send the token to the client
    if let Err(e) = sender.send(Message::Text(token.clone().into())).await {
        error!("Error sending message to {}: {}", addr, e);
    } else {
        info!("Token sent to {}: {}", addr, token);
    }

    // Handle incoming messages
    while let Some(msg) = receiver.next().await {
        match msg {
            Ok(Message::Text(text)) => {
                // ideally each message would bring along data indicating client status to prevent hacker fuckery
                // first digit denotes client screen status
                // after reading the first digit get rid of it and pass the rest of the message into the relevant function
                    // 1 = start screen
                    // 2 = sign up screen
                    // 3 = main app (the scanning screen)
                    // 4 = payment screen
                    // 5 = transferring to till (either by choice or to check id)
                    // 6 = after payment/logging out

                // get first char
                let first_char = text.chars().next().unwrap();

                // get the rest of the string
                let msg = text.chars().next().map(|c| &text[c.len_utf8()..]).unwrap().to_string();

                match first_char {
                    '1' => info!("passing \"{}\" into start screen func", msg),
                    _ => {error!("lol")}
                }





                // Reverse the received string and send it back
                info!("Received from {}: {}", addr, text);
                let reversed = text.chars().rev().collect::<String>();
                if let Err(e) = sender.send(Message::Text(reversed.clone().into())).await {
                    error!("Error sending message to {}: {}", addr, e);
                } else {
                    info!("Sent to {}: {}", addr, reversed);
                }
            }
            Ok(Message::Binary(_text)) => {
                // binary strings are not supported and should not be sent from the front end anyways
                // no to mention sending binary in react syntax is send("the string") compared to
                // binary which is send(new Blob(["the string"]))

                if let Err(e) = sender.send(Message::Text(Utf8Bytes::from("are u hacking me UWU"))).await {
                    error!("Error sending message to {}: {}", addr, e);
                } else {
                    info!("Sent to {}: are u hacking me UWU", addr);
                }
            }
            Ok(Message::Close(_)) => {
                let mut list = list_lock.lock().unwrap();
                for i in 0..list.len() - 1 {
                    if list[i].addr == addr {
                        list.remove(i);
                        info!("disconnected connection with {}", addr);
                        debug!("{:#?}", list);
                    }
                }
                break
            },
            Ok(_) => (),
            Err(e) => {
                error!("Error processing message from {}: {}", addr, e);
                break;
            }
        }
    }
}

fn token_gen(list: &Vec<ConnectionInfo>) -> String {
    // function for generating user tokens
    // will check if the token is used before returning it

    let len = 10;
    let mut output = String::new();
    const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    loop {
        let mut rng = rand::thread_rng();
        let one_char = || CHARSET[rng.gen_range(0..CHARSET.len())] as char;
        output = iter::repeat_with(one_char).take(len).collect();
        let mut taken = false;
        for li in list {
            if li.token == output{
                taken = true
            }
        }
        if !taken { break; }
    }

    output
}