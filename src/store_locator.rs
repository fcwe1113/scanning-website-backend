use std::{future::Future, net::SocketAddr, sync::Arc};
use anyhow::{bail, Error};
use futures_util::SinkExt;
use futures_util::stream::SplitSink;
use log::{debug, error, info};
use tokio::{net::TcpStream, sync::{Mutex, RwLock}};
use tokio_tungstenite::WebSocketStream;
use tungstenite::Message;
use serde::Serialize;
use tokio_rustls::server::TlsStream;
use crate::{client_connection::update_nonce, connection_info::ConnectionInfo, screen_state::ScreenState};

#[derive(Clone, Debug, Serialize)]
pub(crate) struct ShopInfo {
    pub(crate) name: String,
    pub(crate) address: String
}

impl ShopInfo {
    // fn new_empty() -> ShopInfo {
    //     ShopInfo {
    //         name: String::new(),
    //         address: String::new()
    //     }
    // }
}

pub(crate) async fn store_locator_handler(
    msg: &mut String,
    sender: &mut SplitSink<WebSocketStream<TlsStream<TcpStream>>, Message>,
    addr: &SocketAddr,
    token: &String,
    nonce: &mut String,
    username: &String,
    status_check_timer: &mut i32,
    list_lock: Arc<Mutex<Vec<ConnectionInfo>>>,
    shop_list: Arc<RwLock<Vec<ShopInfo>>>,
    shop_id: &mut i32,
) -> Result<(), Error> {

    // 3 = store locator
        // a. check items: token, username
        // b. do regular status checks until user scans/inputs a store***
        // c. client asks for the list of stores with "3LIST"***
        // d. server pings down the list of stores, in a json that is just an array of jsons with the header of "list" appended by "3LIST"
            // each array member will contain "shop_list" and "address"
        // e. client then displays the full store list in a drop down***
            // the qr code would contain a valid copy of the store list entry the qr code is for
            // if user scans before step c is complete store the scanned value in a buffer and then check against it when step c is done***
        // f. client then double checks with user on store selection and sends the id of the store as "3STORE[id]" and also stores the store id***
        // g. server sends an ACK
        // h. client moves onto the main app***

    let result = store_locator_screen(msg, sender, addr, token, nonce, status_check_timer, shop_list, shop_id, username);
    if let Err(err) = resolve_result(result, addr, list_lock.clone()).await {
        bail!(err);
    }

    Ok(())

}

async fn store_locator_screen(
    msg: &mut String,
    sender: &mut SplitSink<WebSocketStream<TlsStream<TcpStream>>, Message>,
    addr: &SocketAddr,
    token: &String,
    nonce: &mut String,
    status_check_timer: &mut i32,
    shop_list: Arc<RwLock<Vec<ShopInfo>>>,
    shop_id: &mut i32,
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
    } else if msg.chars().take(4).collect::<String>() == "NEXT" {
        if msg.chars().skip(4).collect::<String>() == *shop_id.to_string() {
            if let Err(e) = sender.send(Message::from("3NEXT")).await{
                bail!("failed to send moving on message in store locator to client {}: {}", addr, e)
            }
            Ok("moving to main app".to_string())
        } else {
            bail!("invalid store ID for {}", addr);
        }
    } else if msg == "ACK" {
        Ok("STATUS ok".to_string())
    } else if msg == "LIST" {
        let mut json_list = String::from("{\"list\": ["); // somewhere here has bad syntax
        for shop in shop_list.read().await.iter() {
            // println!("{}", serde_json::to_string(shop)?);
            json_list = format!("{}{},", json_list, serde_json::to_string(shop)?);
        }

        json_list.pop();
        json_list = format!("{}]}}", json_list);
        if let Err(e) = sender.send(Message::from(format!("3LIST{}", json_list))).await { // 3d. sending list
            bail!("Error sending shop list to {}: {}", addr, e);
        }
        debug!("shop list sent to {}", addr);
        Ok("STATUS ok".to_string())
    } else if msg.chars().take(5).collect::<String>() == "STORE" {
        let temp = msg.chars().skip(5).collect::<String>();
        match temp.parse::<i32>() {
            Ok(id) => {
                *shop_id = id;
                if let Err(e) = sender.send(Message::from("3ACK")).await{ // 3g.
                    bail!("failed to send ack in store locator to client {}: {}", addr, e);
                }
                Ok("STATUS ok".to_string())
            }
            Err(e) => {bail!("client {} sent invalid store id: {}", addr, e)}
        }
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
                    for connection_info in list_lock.lock().await.iter_mut() {
                        if connection_info.client_addr == *addr {
                            connection_info.screen = ScreenState::Scanner;
                        }
                    }
                    info!("moving client {} onto main app", addr);
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