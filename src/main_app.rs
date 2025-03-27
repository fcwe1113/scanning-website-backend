use std::future::Future;
use std::net::SocketAddr;
use std::sync::Arc;
use anyhow::{bail, Error};
use futures_util::SinkExt;
use futures_util::stream::SplitSink;
use log::{debug, error, info};
use rusqlite::Connection;
use rusqlite::fallible_iterator::FallibleIterator;
use serde::{Deserialize, Serialize};
use tokio::net::TcpStream;
use tokio::sync::{Mutex, RwLock};
use tokio::task;
use tokio_tungstenite::WebSocketStream;
use tracing::field::debug;
use tungstenite::Message;
use crate::client_connection::update_nonce;
use crate::connection_info::ConnectionInfo;
use crate::screen_state::ScreenState;

#[derive(Serialize, Deserialize, Debug)]
struct ItemInfo {
    id: i32,
    name: String,
    price: f64,
    age_limit: i32,
    divisible: bool,
    quantity: f64,
}

impl ItemInfo {
    fn new(id: i32, name: String, price: f64, age_limit: i32, divisible: bool, quantity: f64) -> Self {
        ItemInfo { id, name, price, age_limit, divisible, quantity }
    }
}

#[derive(Deserialize)]
pub(crate) struct CheckoutList {
    list: Vec<ItemInfo>,
    total: f64,
}

impl CheckoutList {
    fn new(list: Vec<ItemInfo>, total: f64) -> Self {
        Self { list, total }
    }

    pub(crate) fn new_empty() -> Self {
        Self { list: Vec::new(), total: 0.0 }
    }

    fn update_total(&mut self) {
        self.total = 0.0;
        for item in self.list.iter() {
            self.total += item.quantity * item.price;
        }
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
    checkout_list: &mut CheckoutList,
    db: &mut Connection
) -> Result<(), Error> {

    // 4 = main_app
    // a. check items: token, username, storeid
    // b. do regular status checks until user checkout***
    // c. user is free to scan/type in random barcodes and everytime they do that id gets sent up to the server appended with "4ITEM"***
        // note the id is a positive integer to both ends will need to sanitize it before sending it to server/querying the db with it
        // I. server would look at the id and ping back the item name, price and minimum age in a json in the format of "4ITEM[json]"
            // if the id given is invalid the server would ping "4INVALID"
            // note the server WILL NOT be keeping track of the shopping list in this stage
    // d. client takes in the json and display the item on a list on screen***
    // e. when user clicks the checkout button client sends the entire list of items as a json with quantity up to server like "4CHECKOUT[json]"***
    // f. server sends an ACK
    // g. move on to payment

    let result = main_app_screen(msg, sender, addr, token, nonce, status_check_timer, shop_id, username, checkout_list, db);
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
    checkout_list: &mut CheckoutList,
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
    } else if msg.chars().take(4).collect::<String>() == "ITEM" { // 4cI. checks for received item id

        // technically trying to find a number id with text wont break the system
        // but i dont wanna come fix this if someone typed in the magic hacking words that can somehow take the system down
        let id = match msg.chars().skip(4).collect::<String>().parse::<i32>() {
            Ok(i) => i,
            Err(e) => {
                sender.send(Message::from("4INVALID")).await?; // 4cI. invalid
                error!("client {} sent invalid item id: {}", addr, e);
                return Ok("STATUS ok".to_string())
            }
        };

        let result = task::block_in_place(|| {
            let mut stmt = db.prepare("SELECT id, name, price_per_unit, age_limit, unit FROM Items WHERE id = ?").unwrap();
            return stmt.query_map([&id], |row| {
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
                    quantity: 0.0,
                })
            }).unwrap().collect::<Result<Vec<_>, _>>().unwrap();
        });

        if result.is_empty() { // if the vector is empty that means no item of that id was found
            sender.send(Message::from("4INVALID")).await?; // 4cI. invalid
            debug!("sent item id invalid message to client {}", addr);
        } else {
            // if the query result is not empty that means it succeeded
            sender.send(Message::from(format!("4ITEM{}", serde_json::to_string(&result[0]).unwrap()))).await?;
            debug!("sent data for item id {} to client {}", id, addr);
        }

        Ok("STATUS ok".to_string())
    } else if msg.chars().take(8).collect::<String>() == "CHECKOUT" {

        // info!("checkout list: {}", msg.chars().skip(8).collect::<String>());
        let list: CheckoutList = serde_json::from_str(&msg.chars().skip(8).collect::<String>())?;
        let total = list.total;
        let list: Vec<ItemInfo> = list.list;

        if list.is_empty() { // if client sent empty shopping list than they are a naughty hacker
            bail!("client {} sent empty checkout list", addr);
        }

        // count up the total to see if total amount is correct
        let mut temp_total = 0.0;
        for item in list.iter() {
            let result = task::block_in_place(|| {
                let mut stmt = db.prepare("SELECT id, price_per_unit, unit, age_limit FROM Items WHERE id = ?").unwrap();
                return stmt.query_map([item.id], |row| {
                    Ok(ItemInfo {
                        id: row.get(0)?,
                        name: String::new(),
                        price: row.get(1)?,
                        age_limit: match row.get(3) {
                            Ok(i) => match i {
                                None => 0,
                                _ => i.expect("lol"), // expect is only here to shut up the compiler
                            },
                            _ => { 0 }
                        },
                        divisible: match row.get::<usize, String>(2) {
                            Ok(_) => true,
                            _ => false
                        },
                        quantity: item.quantity,
                    })
                }).unwrap().collect::<Result<Vec<_>, _>>().unwrap();
            });
            if result.is_empty() { // if client sent nonexistent item id that means they are a naughty hacker
                bail!("client {} sent invalid item id: {}", addr, item.id);
            } else if item.price != result[0].price || item.divisible != result[0].divisible || item.age_limit != result[0].age_limit {
                // if the item data is modified means they are a naughty hacker
                bail!("client {} sent modified item data for id {}", addr, item.id);
            }
            temp_total += item.price * item.quantity;

        }

        if temp_total != total { // if client sent wrong total then they are a naughty hacker
            bail!("client {} sent incorrect total: temp: {} correct: {}", addr, total, temp_total);
        }

        *checkout_list = CheckoutList::new(list, total);
        sender.send(Message::from("4ACK")).await?; // 4f. ack

        Ok("STATUS ok".to_string())
    } else if msg.chars().take(4).collect::<String>() == "NEXT" { // 4g. moving on
        if msg.chars().skip(4).collect::<String>() == checkout_list.total.to_string() {
            sender.send(Message::from("4NEXT")).await?;
            Ok("moving to payment".to_string())
        } else {
            bail!("invalid total amount for client {}", addr);
        }
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