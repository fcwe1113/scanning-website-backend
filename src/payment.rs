use std::future::Future;
use std::net::SocketAddr;
use std::sync::Arc;
use anyhow::{bail, Error};
use chrono::{Local, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use creditcard::CreditCard;
use futures_util::SinkExt;
use futures_util::stream::SplitSink;
use log::{debug, error, info};
use rusqlite::Connection;
use serde::Deserialize;
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio::task;
use tokio_tungstenite::WebSocketStream;
use tungstenite::Message;
use regex::Regex;
use rusqlite::fallible_iterator::FallibleIterator;
use rusqlite::types::Value;
use warp::hyper::body::HttpBody;
use crate::client_connection::update_nonce;
use crate::connection_info::ConnectionInfo;
use crate::main_app::{CheckoutList, ItemInfo};
use crate::screen_state::ScreenState;

#[derive(Deserialize)]
struct CardInfo {
    number: String,
    expiry: String,
    cvv: String,
}

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
    db: &mut Connection,
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
    // h. if user clicks transfer to till (willing or not) client has to scan a qr code and send "5TRANSFER[till token]"
    // j. backend just takes that in and replies "5OK", if sent an invalid till token reply "5INVALID"

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
        if checkout_list.force_till() {
            bail!("client {} refuses to use till for age limited items, exiting", addr);
        }
        info!("client {} paid via apple pay", addr);
        checkout(checkout_list, username, shop_id, db).await;
        sender.send(Message::from("5SUCCESS")).await?;
        Ok("STATUS ok".to_string())
    } else if msg == "GOOGLE" {
        if checkout_list.force_till() {
            bail!("client {} refuses to use till for age limited items, exiting", addr);
        }
        info!("client {} paid via google pay", addr);
        checkout(checkout_list, username, shop_id, db).await;
        sender.send(Message::from("5SUCCESS")).await?;
        Ok("STATUS ok".to_string())
    } else if msg.chars().take(8).collect::<String>() == "TRANSFER" {
        let token = msg.chars().skip(8).collect::<String>();
        if token.len() != 10 { // just say the token has to be 10 long for now
            info!("client {} gave invalid till token", addr);
            sender.send(Message::from("5INVALID")).await?;
        } else {
            info!("client {} transferred to till via token {}", addr, token);
            sender.send(Message::from("5OK")).await?;
        }
        Ok("STATUS ok".to_string())
    } else if msg.chars().take(4).collect::<String>() == "CARD" {

        if checkout_list.force_till() {
            bail!("client {} refuses to use till for age limited items, exiting", addr);
        }

        info!("received card info: {}", msg.chars().skip(4).collect::<String>());
        let card: CardInfo = serde_json::from_str(&msg.chars().skip(4).collect::<String>())?;

        let mut error = false;
        match card.number.parse::<CreditCard>() {
            Ok(_) => {}
            Err(_) => {
                sender.send(Message::from("5INVALID")).await?;
                error = true;
            }
        }
        if error { // stop the rest of the code if the card nnumber is invalid
            return Ok("STATUS ok".to_string())
        }
        
        if Regex::new("^\\d{2}/\\d{4}$")?.is_match(card.expiry.as_str()) { 
            if card.expiry.chars().take(2).collect::<String>().parse::<i32>()? < 12 { 
                if NaiveDateTime::new(
                    NaiveDate::from_ymd_opt(card.expiry.chars().skip(3).collect::<String>().parse::<i32>()?,
                                            card.expiry.chars().take(2).collect::<String>().parse::<u32>()?, 1).unwrap(),
                    NaiveTime::from_hms_opt(0, 0, 0).unwrap()).and_utc().timestamp() - Utc::now().timestamp() >= 0 {
                        if Regex::new("^\\d{3}$")?.is_match(card.cvv.as_str()) {
                            checkout(checkout_list, username, shop_id, db).await;
                            info!("client {} paid by card", addr);
                            return Ok("STATUS ok".to_string())
                        }
                }
            }
        }
        
        bail!("invalid credit card past browser sanitation submitted by client {}", addr);
    } else {
        bail!("payment screen received invalid message from {}: {}", addr, msg);
    }
}

async fn resolve_result(result: impl Future<Output=Result<String, Error>>+Sized, addr: &SocketAddr, list_lock: Arc<Mutex<Vec<ConnectionInfo>>>) -> Result<(), Error> {
    match result.await {
        Ok(r) => {
            // debug!("result: {}", r.as_str());
            match r.as_str() {

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

async fn checkout(list: &CheckoutList, username: &String, shop_id: &i32, db: &mut Connection) {
    let shop_id = shop_id + 1;
    let tx = db.transaction().unwrap();
    let time = &NaiveDateTime::from(Local::now().naive_local()).to_string();
    // db.execute("BEGIN TRANSACTION"[]).unwrap();
    tx.execute("INSERT INTO Transactions (username, shop_id, time) VALUES ($1, $2, $3);", [
        Value::from(username.clone()),
        Value::from(shop_id),
        Value::from(time.clone()),
    ]).unwrap();

    let mut id = tx.prepare("SELECT id FROM Transactions WHERE time = $1;").unwrap();
    let id = task::block_in_place(move || {id.query_map([time], |row| {
        Ok(row.get(0).unwrap())
    }).unwrap().collect::<Result<Vec<i32>, _>>().unwrap()[0]});

    for item in &list.list {

        // update purchase records
        tx.execute("INSERT INTO Purchase_records (transaction_id, item_id, quantity) VALUES ($1, $2, $3);", [
            Value::from(id),
            Value::from(item.id),
            Value::from(item.quantity)
        ]).unwrap();

        // update shop stocks, would have no effect on null values or nonexistent records
        tx.execute("UPDATE Shop_stock set stock = stock - $1 where shop_id = $2 and item_id = $3;", [
            Value::from(item.quantity),
            Value::from(shop_id),
            Value::from(item.id)
        ]).unwrap();

    }

    // db.execute("COMMIT;", []).unwrap();
    tx.commit().unwrap();
}