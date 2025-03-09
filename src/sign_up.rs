use std::future::Future;
use std::net::SocketAddr;
use std::sync::Arc;
use anyhow::{bail, Error};
use chrono::Duration;
use futures_util::SinkExt;
use futures_util::stream::SplitSink;
use log::{debug, error, info};
use rand::Rng;
use rand_chacha::ChaCha20Rng;
use rand_chacha::rand_core::SeedableRng;
use rusqlite::Connection;
use timer::Timer;
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio_rustls::server::TlsStream;
use tokio_tungstenite::WebSocketStream;
use tungstenite::Message;
use crate::connection_info::ConnectionInfo;
use crate::screen_state::ScreenState;

pub(crate) async fn sign_up_handler( // handler function for the start screen
                                     msg: &mut String,
                                     sender: &mut SplitSink<WebSocketStream<TlsStream<TcpStream>>, Message>,
                                     addr: &SocketAddr,
                                     token: &String,
                                     nonce: &mut String,
                                     username: &mut String,
                                     timer: &Timer,
                                     list_lock: Arc<Mutex<Vec<ConnectionInfo>>>,
                                     db: &mut Connection
) -> Result<(), Error>{

    // 2 = start screen
        // a. check items: token
        // b. do regular status checks until user either clicks log in sign up or proceed as guest***
        // c. if user logs in client sends username and password in textbox***
        // with the format "1username password"
            // I. server querys db to get password of username
            // II. server saves username locally and pings down OK if correct
            // if db returns incorrect or empty pings down FAIL and returns to step 1b.
            // III. client saves the username locally and pings "1NEXT3" to server***, server go to step 1f.
        // d. if user clicks sign up client pings "1NEXT2"***, server go to step 1f.
        // e. if user clicks proceed as guest client pings "1guest 00000000" to server***
            // I. server saves the username locally and pings "1ACK" to client
            // II. client saves username locally and pings "1NEXT 3 token"***, server go to step 1f.
        // f. server decipher the message, checks the token to be correct,
        // and extract the destination screen status contained in it
        // g. server pings "1NEXT *2/3*" depending on which one the client sent before
        // and server moves on to that state
        // h. client receives message and also moves on to the next state

    // debug!("Starting screen handler received {}", msg);
    // println!("{}", msg.chars().take(5).collect::<String>());
    // println!("{}", msg);
    let result = sign_up_screen(msg, sender, addr, token, nonce, timer, db, username);
    if let Err(err) = resolve_result(result, addr, list_lock.clone()).await {
        bail!(err);
    }

    Ok(())

}

async fn sign_up_screen(
    msg: &mut String,
    sender: &mut SplitSink<WebSocketStream<TlsStream<TcpStream>>, Message>,
    addr: &SocketAddr,
    token: &String,
    nonce: &mut String,
    timer: &Timer,
    db: &mut Connection,
    username: &mut String
) -> Result<String, Error> {
    if msg.chars().take(6).collect::<String>() == "STATUS" {
        // messages starting with STATUS denotes that this is a regular status check
        let msg = msg.chars().skip(6).collect::<String>();
        if msg == *token {
            // resets the timer when the status check is received
            debug!("status checked for {}", addr);
            timer.schedule_with_delay(Duration::minutes(3), move || { return; });
            // generate a new nonce and send it over
            let mut rng = ChaCha20Rng::from_os_rng();
            *nonce = (0..20).map(|_| char::from(rng.random_range(32..127))).collect::<String>();
            if let Err(e) = sender.send(Message::from(format!("STATUS{}", nonce))).await {
                bail!("failed to send nonce to {}: {}", addr, e);
            } else {
                info!("updated nonce sent to {}: {}", addr, nonce);
                // let _ = Ok::<String, String>("nonce updated".to_string());
                Ok("STATUS ok".to_string())
            }
        } else {
            bail!("invalid status check for {}", addr);
        }
    } else {
        bail!("sign up screen received invalid message from {}: {}", addr, msg);
    }
}

async fn resolve_result(result: impl Future<Output=Result<String, Error>> + Sized, addr: &SocketAddr, list_lock: Arc<Mutex<Vec<ConnectionInfo>>>) -> Result<(), Error> {
    match result.await {
        Ok(r) => {
            // debug!("result: {}", r.as_str());
            match r.as_str() {

                "moving to store locator" => {
                    for connection_info in list_lock.lock().await.iter_mut() {
                        if connection_info.client_addr == *addr {
                            connection_info.screen = ScreenState::StoreLocator;
                        }
                    }
                    info!("moving client {} onto store locator screen", addr);
                    Ok(())
                },
                "STATUS ok" => {
                    Ok(()) // do nothing
                }
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