use std::future::Future;
use std::net::SocketAddr;
use std::sync::Arc;
use anyhow::{bail, Error};
use futures_util::SinkExt;
use futures_util::stream::SplitSink;
use log::{debug, error, info};
use rusqlite::Connection;
use rusqlite::fallible_iterator::FallibleIterator;
use serde::Serialize;
use tokio::net::TcpStream;
use tokio::sync::{Mutex, RwLock};
use tokio::task;
use tokio_tungstenite::WebSocketStream;
use tungstenite::Message;
use crate::client_connection::update_nonce;
use crate::connection_info::ConnectionInfo;
use crate::screen_state::ScreenState;

#[derive(Serialize)]
struct ItemInfo {
    id: i32,
    name: String,
    price: f64,
    age_limit: i32,
    divisible: bool,
}

impl ItemInfo {
    fn new(id: i32, name: String, price: f64, age_limit: i32, divisible: bool) -> Self {
        ItemInfo { id, name, price, age_limit, divisible }
    }
}

pub(crate) async fn main_app_handler(
    msg: &mut String,
    sender: &mut SplitSink<WebSocketStream<TcpStream>, Message>,
    addr: &SocketAddr,
    token: &String,
    nonce: &mut String,
    username: &String,
    status_check_timer: &mut i32,
    list_lock: Arc<Mutex<Vec<ConnectionInfo>>>,
    shop_id: &mut i32,
    db: &mut Connection
) -> Result<(), Error> {

    // 4 = main_app
    // a. check items: token, username, storeid
    // b. do regular status checks until user scans/inputs a store***
    // c. user is free to scan/type in random barcodes and everytime they do that id gets sent up to the server appended with "4ITEM"
        // note the id is a positive integer to both ends will need to sanitize it before sending it to server/querying the db with it
        // I. server would look at the id and ping back the item name, price and minimum age in a json in the format of "4ITEM[json]"
            // if the id given is invalid the server would ping "4INVALID"
            // note the server WILL NOT be keeping track of the shopping list in this stage
    // d. client takes in the json and display the item on a list on screen
    // e. when user clicks the checkout button client sends the entire list of items as a json with quantity up to server like "4CHECKOUT[json]"
    // f. server sends an ACK
    // g. move on to payment

    let result = main_app_screen(msg, sender, addr, token, nonce, status_check_timer, shop_id, username, db);
    if let Err(err) = resolve_result(result, addr, list_lock.clone()).await {
        bail!(err);
    }

    Ok(())

}

async fn main_app_screen(
    msg: &mut String,
    sender: &mut SplitSink<WebSocketStream<TcpStream>, Message>,
    addr: &SocketAddr,
    token: &String,
    nonce: &mut String,
    status_check_timer: &mut i32,
    shop_id: &mut i32,
    username: &String,
    db: &mut Connection) -> Result<String, Error> {

    if msg.chars().take(6).collect::<String>() == "STATUS" {
        // messages starting with STATUS denotes that this is a regular status check
        let msg = msg.chars().skip(6).collect::<String>();
        if msg == format!("{}{}{}", *token, username, shop_id) { // 2a. checking token
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
    } else if msg.chars().take(4).collect::<String>() == "ITEM" {

        // technically trying to find a number id with text wont break the system
        // but i dont wanna come fix this if someone typed in the magic hacking words that can somehow take the system down
        let id = match msg.chars().skip(4).collect::<String>().parse::<i32>() {
            Ok(i) => i,
            Err(e) => {
                sender.send(Message::from("4INVALID")).await?;
                error!("client {} sent invalid item id: {}", addr, e);
                return Ok("STATUS ok".to_string())
            }
        };

        let result = task::block_in_place(|| { // 2f. check username against db
            let mut stmt = db.prepare("SELECT id, name, price_per_unit, age_limit, unit FROM Items WHERE id = ?").unwrap();
            let query_iter = stmt.query_map([&id], |row| {
                Ok(ItemInfo {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    price: row.get(2)?,
                    age_limit: match row.get(3) {
                        Ok(i) => match i {
                            None => 0,
                            _ => i.expect("lol"), // todo may not be functional
                        },
                        _ => { 0 }
                    },
                    divisible: match row.get::<usize, String>(4) {
                        Ok(_) => true,
                        _ => false
                    },
                })
            }).unwrap();
            return query_iter.collect::<Result<Vec<_>, _>>().unwrap()
        });

        if result.is_empty() { // if the vector is empty that means no item of that id was found
            sender.send(Message::from("4INVALID")).await?;
            debug!("sent item id invalid message to client {}", addr);
        } else {
            // if the query result is not empty that means it suceeded
            sender.send(Message::from(format!("4ITEM{}", serde_json::to_string(&result[0]).unwrap()))).await?;
            debug!("sent data for item id {} to client {}", id, addr);
        }

        Ok("STATUS ok".to_string())
    } else {
        bail!("main app screen received invalid message from {}: {}", addr, msg);
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