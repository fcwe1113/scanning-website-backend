use std::future::Future;
use std::net::SocketAddr;
use std::sync::Arc;
use anyhow::{bail, Error};
use futures_util::SinkExt;
use futures_util::stream::SplitSink;
use log::{debug, error, info};
use rusqlite::Connection;
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio::task;
use tokio_tungstenite::WebSocketStream;
use tungstenite::Message;
use crate::client_connection::update_nonce;
use crate::connection_info::ConnectionInfo;
use crate::main_app::{CheckoutList, ItemInfo};
use crate::screen_state::ScreenState;

pub(crate) async fn payment_handler(
    msg: &mut String,
    sender: &mut SplitSink<WebSocketStream<TcpStream>, Message>,
    addr: &SocketAddr,
    token: &String,
    nonce: &mut String,
    username: &String,
    status_check_timer: &mut i32,
    list_lock: Arc<Mutex<Vec<ConnectionInfo>>>,
    shop_id: &mut i32,
    checkout_list: &mut CheckoutList,
    db: &mut Connection
) -> Result<(), Error> {

    // 5 = payment
    // a. check items: token, username, storeid, checkout total
    // b. do regular status checks until user checkout***
    // c. user is given the list of groceries again along with the total cost of it all, and 2 payment methods, card info or apple/google pay
    // d. if user clicks on card payment frontend would send in the given card details, or in a dropdown if there is one stored in db
        // client sends card payment as "5CARD[card json]" and apple/google pay as "5GOOGLE"/"5APPLE"
    // e. backend processes the payment,
        // if its apple/google pay just responds complete
        // if by card it checks field validity then responds complete
    // f. backend saves the checkout list in db
    // g. if backend responds "5SUCCESS" front end show success message, if a "5FAIL" is sent show an error and let the user try again
    // h. if user clicks transfer to till (willing or not) client sends "3TRANSFER"
    // i. client then has to scan a qr code on their (imaginary) till and it contains "5TILL[till token]"
    // j. backend just takes that in and replies "5OK"

    let result = payment_screen(msg, sender, addr, token, nonce, status_check_timer, shop_id, username, checkout_list, db);
    if let Err(err) = resolve_result(result, addr, list_lock.clone()).await {
        bail!(err);
    }

    Ok(())

}

async fn payment_screen(
    msg: &mut String,
    sender: &mut SplitSink<WebSocketStream<TcpStream>, Message>,
    addr: &SocketAddr,
    token: &String,
    nonce: &mut String,
    status_check_timer: &mut i32,
    shop_id: &mut i32,
    username: &String,
    checkout_list: &mut CheckoutList,
    db: &mut Connection) -> Result<String, Error> {

    if msg.chars().take(6).collect::<String>() == "STATUS" {
        // messages starting with STATUS denotes that this is a regular status check
        let msg = msg.chars().skip(6).collect::<String>();
        if msg == format!("{}{}{}{}", *token, username, shop_id, checkout_list.total) { // 2a. checking token
            // resets the timer when the status check is received
            debug!("status checked for {}", addr);
            *status_check_timer = 0;
            match update_nonce(nonce, sender, addr).await {
                Ok(s) => { Ok(s) }
                Err(e) => { bail!(e); }
            }
        } else {
            bail!("invalid status check for {}", addr);
        }
    } else if msg == "APPLE" {
        info!("client {} paid via apple pay", addr);
        // todo count stock
        sender.send(Message::from("5SUCCESS")).await?;
        Ok("STATUS ok".to_string())
    } else if msg == "GOOGLE" {
        info!("client {} paid via google pay", addr);
        // todo count stock
        sender.send(Message::from("5SUCCESS")).await?;
        Ok("STATUS ok".to_string())
    } else {
        bail!("payment screen received invalid message from {}: {}", addr, msg);
    }
}

async fn resolve_result(result: impl Future<Output=Result<String, Error>>+Sized, addr: &SocketAddr, list_lock: Arc<Mutex<Vec<ConnectionInfo>>>) -> Result<(), Error> {
    match result.await {
        Ok(r) => {
            // debug!("result: {}", r.as_str());
            match r.as_str() {

                "moving to payment" => {
                    for connection_info in list_lock.lock().await.iter_mut() {
                        if connection_info.client_addr == *addr {
                            connection_info.screen = ScreenState::Payment;
                        }
                    }
                    info!("moving client {} onto payment", addr);
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