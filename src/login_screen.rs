use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use anyhow::{bail, Error};
use chrono::Duration;
use futures_util::stream::SplitSink;
use log::debug;
use timer::Timer;
use tokio::net::TcpStream;
use tokio_tungstenite::WebSocketStream;
use tungstenite::Message;
use crate::connection_info::ConnectionInfo;

pub(crate) async fn start_screen_handler(
    msg: String,
    sender: &mut SplitSink<WebSocketStream<TcpStream>, Message>,
    addr: &SocketAddr,
    token: &String,
    nonce: &mut String,
    timer: &Timer,
    list_lock: Arc<Mutex<Vec<ConnectionInfo>>>
) -> Result<(), Error>{

    // 1 = start screen
        // a. check items: token
        // b. do regular status checks until user either clicks log in sign up or proceed as guest***
        // c. if user logs in client sends username and password in textbox***
        // with the format "1username password"
            // I. server querys db to get password of username
            // II. server saves username locally and pings down OK if correct
            // if db returns incorrect or empty pings down BADINFO and returns to step 1b.
            // III. client saves the username locally and pings "1NEXT 3 token" to server***, server go to step 1f.
        // d. if user clicks sign up client pings "1NEXT 2 token"***, server go to step 1f.
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
    let result = start_screen(&mut msg.clone(), sender, addr, &token, nonce, timer, list_lock.clone());

    // if let Err(e) = crate::token_exchange::resolve_result(result, token_exchanged, addr, token, list_lock).await{
    //     bail!(e);
    // }

    Ok(())
    
}

fn start_screen(
    msg: &mut String,
    sender: &mut SplitSink<WebSocketStream<TcpStream>, Message>,
    addr: &SocketAddr,
    token: &String,
    nonce: &mut String,
    timer: &Timer,
    list_lock: Arc<Mutex<Vec<ConnectionInfo>>>
) -> Result<String, Error>{
    if msg.chars().take(6).collect::<String>() == "STATUS" {
        let msg = msg.chars().skip(6).collect::<String>();
        if msg == *token {
            // resets the timer when the status check is received
            debug!("status checked for {}", addr);
            timer.schedule_with_delay(Duration::minutes(3), move || {return;});
        }
    } else {
        bail!("login screen received invalid message from {}: {}", addr, msg);
    }

    Ok("STATUS ok".to_string())
}