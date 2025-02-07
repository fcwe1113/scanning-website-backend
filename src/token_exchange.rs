use std::iter;
// use std::io::Error;
use std::net::SocketAddr;
use anyhow::{bail, Error};
use futures_util::SinkExt;
use futures_util::stream::SplitSink;
use rand::Rng;
use tokio::net::TcpStream;
use tokio_tungstenite::WebSocketStream;
use tungstenite::{Message, Utf8Bytes};
use crate::connection_info::ConnectionInfo;

pub(crate) async fn token_exchange(msg: String, token: &String, mut sender: &SplitSink<WebSocketStream<TcpStream>, Message>, addr: &SocketAddr, flag: &bool) -> Result<(), Error> {

    if (msg == "NEXT") {
        if *flag == true { // 0e check flag
            if let Err(e) = sender.send(Message::Text(Utf8Bytes::from("0ACK"))).await /*0e ack*/ {}
            Ok("moving on".into())
        } else {
            bail!("client tries to move to start screen before token was exchanged") // 0e error handling
        }
    } else if (msg == *token){ // 0c check token
        if let Err(e) = sender.send(Message::Text(Utf8Bytes::from("0ACK"))).await /*0c ack*/ {
            bail!("failed to send token ack message to {}: {}", addr, e);
        } else {
            Ok("token ackked".into())
        }
    } else {
        bail!("client token mismatch, are you a naughty hacker?")
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