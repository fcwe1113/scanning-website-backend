use std::future::Future;
use std::iter;
// use std::io::Error;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use anyhow::{bail, Error};
use async_channel::Sender;
use futures_util::SinkExt;
use futures_util::stream::SplitSink;
use log::{debug, error, info};
use rand::Rng;
use tokio::net::TcpStream;
use tokio_rustls::server::TlsStream;
use tokio_tungstenite::WebSocketStream;
use tungstenite::{Message, Utf8Bytes};
use crate::connection_info::ConnectionInfo;
use crate::screen_state::ScreenState;

// messages sent from both ends should follow a similar format (at least for the first few chars)
// *** denotes client side tasks
// 0 = token exchange
    // a. when the websocket channel opens the server will generate the token and send it to the client, the client would have a placeholder token which prevent the client from proceeding
    // b. the client saves the token and pings back the same token to the server***
    // c. the server sends an ack back if the token matches and switches the token_exchanged flag on and saves it in the list(tm)
    // d. client then tells the server to move on to the start screen state***
    // e. server tells client to move on then moves on itself, unless the token_exchanged flag is not on, in which case handle the error
    // f. client moves on for real***
    // while the client is waiting for the token exchange ack it can show a loading wheel or something idk

pub(crate) async fn token_exchange_handler(msg: String, sender: &mut SplitSink<WebSocketStream<TcpStream>, Message>, token_exchanged: &mut bool, addr: &SocketAddr, token: &String, list_lock: Arc<Mutex<Vec<ConnectionInfo>>>) -> Result<(), Error>{

    // rust does not allow &mut var to be passed into funcs as &var so we cop out and clone the guy
    // we only needed to read the value anyways
    let flag = token_exchanged.clone();
    let result = token_exchange(msg.clone(), &token, sender, addr, &flag);

    if let Err(e) = resolve_result(result, token_exchanged, addr, token, list_lock).await{
        bail!(e);
    }

    Ok(())
}

async fn token_exchange(msg: String, token: &String, sender: &mut SplitSink<WebSocketStream<TcpStream>, Message>, addr: &SocketAddr, flag: &bool) -> Result<String, Error> {

    if (msg == "NEXT") {
        // debug!("{}", flag.to_string());
        if *flag == true { // 0e check flag
            if let Err(_e) = sender.send(Message::Text(Utf8Bytes::from("0NEXT"))).await /*0e ack*/ {}
            Ok(String::from("moving on"))
        } else {
            bail!("client {addr} tries to move to start screen before token was exchanged") // 0e error handling
        }
    } else if (msg == *token){ // 0c check token
        if let Err(e) = sender.send(Message::Text(Utf8Bytes::from("0ACK"))).await /*0c ack*/ {
            bail!("failed to send token ack message to {}: {}", addr, e);
        } else {
            Ok(String::from("token ackked"))
        }
    } else {
        bail!("client {addr} token mismatch, are you a naughty hacker?")
    }

}

async fn resolve_result(result: impl Future<Output=Result<String, Error>> + Sized, token_exchanged: &mut bool, addr: &SocketAddr, token: &String, list_lock: Arc<Mutex<Vec<ConnectionInfo>>>) -> Result<(), Error> {
    match result.await {
        Ok(r) => {
            // debug!("result: {}", r.as_str());
            match r.as_str() {

                "token ackked" => {
                    debug!("Token ackked");
                    *token_exchanged = true; // 0c flag
                    debug!("{:#?}", token_exchanged);
                    for connection in list_lock.lock().unwrap().iter_mut() {
                        if connection.client_addr == *addr {
                            connection.token = token.clone(); // 0c saves on list
                        }
                    }
                    Ok(())
                }, //0c
                "moving on" => { // 0e moving on todo
                    for connection_info in list_lock.lock().unwrap().iter_mut() {
                        if connection_info.client_addr == *addr {
                            connection_info.screen = ScreenState::Start;
                        }
                    }
                    info!("moving client {} onto start screen", addr);
                    Ok(())
                },
                _ => {
                    // this will only happen if i fuck up lol
                    error!("why did i receive the message \"{}\"", r.as_str());
                    bail!("how did this happen lol");
                }
            }
        },
        Err(e) => {
            bail!(e);
        }
    }
}

pub(crate) fn token_gen(list: &Vec<ConnectionInfo>) -> String {
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