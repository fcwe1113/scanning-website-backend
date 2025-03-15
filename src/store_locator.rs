use std::fmt;
use std::fmt::Formatter;
use std::future::Future;
use std::net::SocketAddr;
use std::sync::Arc;
use anyhow::{bail, Error};
use futures_util::SinkExt;
use futures_util::stream::SplitSink;
use log::{debug, error, info};
use rand::Rng;
use rand_chacha::ChaCha20Rng;
use rand_chacha::rand_core::SeedableRng;
use rusqlite::Connection;
use serde_json::Value;
use tokio::net::TcpStream;
use tokio::sync::{Mutex, RwLock};
use tokio_tungstenite::WebSocketStream;
use tracing_subscriber::fmt::format;
use tungstenite::Message;
use serde::Serialize;
use crate::client_connection::update_nonce;
use crate::connection_info::ConnectionInfo;
use crate::screen_state::ScreenState;

#[derive(Clone, Debug, Serialize)]
pub(crate) struct ShopInfo {
    pub(crate) name: String,
    pub(crate) address: String
}

impl ShopInfo {
    fn new_empty() -> ShopInfo {
        ShopInfo {
            name: String::new(),
            address: String::new()
        }
    }
}

pub(crate) async fn store_locator_handler(
    msg: &mut String,
    sender: &mut SplitSink<WebSocketStream<TcpStream>, Message>,
    addr: &SocketAddr,
    token: &String,
    nonce: &mut String,
    username: &String,
    status_check_timer: &mut i32,
    list_lock: Arc<Mutex<Vec<ConnectionInfo>>>,
    shop_list: Arc<RwLock<Vec<ShopInfo>>>,
    db: &mut Connection
) -> Result<(), Error> {

    // 3 = store locator
        // a. check items: token, username
        // b. do regular status checks until user scans/inputs a store***
        // c. client asks for the list of stores with "3LIST"***
        // d. server pings down the list of stores, in a json that is just an array of jsons with the header of "list" appended by "3LIST"
            // each array member will contain "shop_list" and "address"
        // e. client then displays the full store list in a drop down***
            // if user scans before step c is complete store the scanned value in a buffer and then check against it when step c is done***
        // f. client then double checks with user on store selection and sends the id of the store as "3STORE[id]" and also stores the store id***
        // g. server sends an ACK
        // h. client moves onto the main app***

    let result = store_locator_screen(msg, sender, addr, token, nonce, status_check_timer, shop_list, username);
    if let Err(err) = resolve_result(result, addr, list_lock.clone()).await {
        bail!(err);
    }

    Ok(())

}

async fn store_locator_screen(
    msg: &mut String,
    sender: &mut SplitSink<WebSocketStream<TcpStream>, Message>,
    addr: &SocketAddr,
    token: &String,
    nonce: &mut String,
    status_check_timer: &mut i32,
    shop_list: Arc<RwLock<Vec<ShopInfo>>>,
    username: &String) -> Result<String, Error> {

    if msg.chars().take(6).collect::<String>() == "STATUS" {
        // messages starting with STATUS denotes that this is a regular status check
        let msg = msg.chars().skip(6).collect::<String>();
        if msg == format!("{}{}", *token, username) { // 2a. checking token
            // resets the timer when the status check is received
            debug!("status checked for {}", addr);
            *status_check_timer = 0;
            match update_nonce(nonce, sender, addr).await{
                Ok(s) => { Ok(s) }
                Err(e) => { bail!(e); }
            }
        } else {
            bail!("invalid status check for {}", addr);
        }
    } else if msg == "ACK" {
        Ok("STATUS ok".to_string())
    } else if msg == "LIST" {
        let mut json_list = String::from("{
        \"list\": [\n"); // somewhere here has bad syntax
        for shop in shop_list.read().await.iter() {
            json_list = format!("{}{},", json_list, serde_json::to_string(shop)?);
        }
        json_list.pop();
        json_list = format!("{}]}}", json_list);

        // println!("{}", json_list);
        if let Err(e) = sender.send(Message::from(format!("3LIST{}", json_list))).await {
            bail!("Error sending shop list to {}: {}", addr, e);
        }
        debug!("shop list sent to {}", addr);
        Ok("STATUS ok".to_string())
    } else {
        bail!("store locator screen received invalid message from {}: {}", addr, msg);
    }
}

async fn resolve_result(result: impl Future<Output=Result<String, Error>>+Sized, addr: &SocketAddr, list_lock: Arc<Mutex<Vec<ConnectionInfo>>>) -> Result<(), Error> {
    match result.await {
        Ok(r) => {
            // debug!("result: {}", r.as_str());
            match r.as_str() {

                "moving to main app" => {
                    // todo
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

// async fn get_shop_list(&shop_list: ){
//
// }