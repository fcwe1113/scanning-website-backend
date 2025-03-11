use std::future::Future;
use std::iter;
use std::net::SocketAddr;
use std::sync::Arc;
use anyhow::{bail, Error};
use futures_util::SinkExt;
use futures_util::stream::SplitSink;
use log::{debug, error, info};
use rand::Rng;
use rand_chacha::ChaCha20Rng;
use rand_chacha::rand_core::SeedableRng;
use tokio::{net::TcpStream, sync::Mutex};
use tokio_rustls::server::TlsStream;
use tokio_tungstenite::WebSocketStream;
use tungstenite::{Message, Utf8Bytes};
use crate::connection_info::ConnectionInfo;
use crate::screen_state::ScreenState;

// messages sent from both ends should follow a similar format (at least for the first few chars)
// *** denotes client side tasks
// 0 = token exchange
    // a. when the websocket channel opens the server will generate the key pair and send the public key to the client, (NOT IMPLEMENTED)
    // b. the client receives the public key then generates its own key pair, if the client isnt ready ping over WAIT instead*** (NOT IMPLEMENTED)
    // c. client sends the public key over in plain text*** (NOT IMPLEMENTED)
    // d. the server saves the client public key and sends over the the aes key encrypted with clients public key and client ACKs*** (NOT IMPLEMENTED)
    // EVERYTHING PAST THIS POINT WILL BE DONE WITH RSA ENCRYPTION(unless its too long then AES) (NOT IMPLEMENTED)
    // e. server will generate a token and send it to the client, the client would have a placeholder token which prevent the client from proceeding
    // f. the client saves the token and pings back the same token to the server***
    // g. the server then sends a randomly generated nonce down to the client
    // EVERYTHING PAST THIS POINT WILL HAVE THE NONCE ATTACHED
    // h. client saves the nonce and send a NEXT prefixed with the nonce (*nonce*0NEXT)
    // i. client then tells the server to move on to the start screen state***
    // j. server tells client to move on then moves on itself, unless the token_exchanged flag is not on, in which case handle the error
    // k. client moves on for real***
    // while the client is waiting for the token exchange ack it can show a loading wheel or something idk

pub(crate) async fn token_exchange_handler(
    msg: String,
    sender: &mut SplitSink<WebSocketStream<TlsStream<TcpStream>>, Message>,
    token_exchanged: &mut bool,
    addr: &SocketAddr,
    token: &String,
    nonce: &mut String,
    list_lock: Arc<Mutex<Vec<ConnectionInfo>>>
) -> Result<(), Error>{

    // rust does not allow &mut var to be passed into funcs as &var so we cop out and clone the guy
    // we only needed to read the value anyways
    let flag = token_exchanged.clone();
    let result = token_exchange(msg.clone(), &token, sender, addr, &flag, nonce);

    if let Err(e) = resolve_result(result, token_exchanged, addr, token, list_lock).await{
        bail!(e);
    }

    Ok(())
}

async fn token_exchange(
    msg: String,
    token: &String,
    sender: &mut SplitSink<WebSocketStream<TlsStream<TcpStream>>, Message>,
    addr: &SocketAddr,
    flag: &bool,
    nonce: &mut String
) -> Result<String, Error> {

    if msg == "NEXT" {
        // debug!("{}", flag.to_string());
        if *flag { // 0h check flag
            if let Err(_e) = sender.send(Message::Text(Utf8Bytes::from("0NEXT"))).await /*0h ack*/ {}
            Ok(String::from("moving on"))
        } else {
            bail!("client {addr} tries to move to start screen before token was exchanged") // 0h error handling
        }
    } else if msg == *token { // 0f check token
        let mut rng = ChaCha20Rng::from_os_rng();

        *nonce = (0..20).map(|_| char::from(rng.random_range(32..127))).collect::<String>();

        if let Err(e) = sender.send(Message::from(format!("0{}", nonce))).await /*0f ack*/ {
            bail!("failed to send nonce to {}: {}", addr, e);
        } else {
            info!("nonce sent to {}: {}", addr, nonce);
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
                    *token_exchanged = true; // 0f flag
                    for connection in list_lock.lock().await.iter_mut() {
                        if connection.client_addr == *addr {
                            connection.token = token.clone(); // 0f saves on list
                        }
                    }
                    Ok(())
                },
                "moving on" => { // 0h moving on todo
                    for connection_info in list_lock.lock().await.iter_mut() {
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
        let mut rng = rand::rng();
        let one_char = || CHARSET[rng.random_range(0..CHARSET.len())] as char;
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