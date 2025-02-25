use std::net::SocketAddr;
use std::sync::Arc;
use anyhow::{bail, Error};
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::SaltString;
use chrono::Duration;
use futures_util::SinkExt;
use futures_util::stream::SplitSink;
use log::{debug, error, info};
use rand::{Rng};
use rand_chacha::ChaCha20Rng;
use rand_chacha::rand_core::SeedableRng;
use timer::Timer;
use tokio::{net::TcpStream, sync::Mutex, task};
use tokio_tungstenite::WebSocketStream;
use tungstenite::{Message, Utf8Bytes};
use rusqlite::Connection;
use rusqlite::fallible_iterator::FallibleIterator;
use crate::connection_info::ConnectionInfo;
use crate::test;

struct password{
    // since the db wont store passwords in plain text but in salted hash the way to check is the password is correct would be to:
    // get the salt and salted hash the user input password
    // see if it matches
    hash: String,
    salt: String,
}

pub(crate) async fn start_screen_handler(
    msg: &mut String,
    sender: &mut SplitSink<WebSocketStream<TcpStream>, Message>,
    addr: &SocketAddr,
    token: &String,
    nonce: &mut String,
    timer: &Timer,
    list_lock: Arc<Mutex<Vec<ConnectionInfo>>>,
    db: &mut Connection
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
    // println!("{}", msg);
    let result = start_screen(msg, sender, addr, &token, nonce, timer, db, list_lock.clone());
    info!("{:?}", result.await?);

    // if let Err(e) = crate::token_exchange::resolve_result(result, token_exchanged, addr, token, list_lock).await{
    //     bail!(e);
    // }

    Ok(())
    
}

async fn start_screen(
    msg: &mut String,
    sender: &mut SplitSink<WebSocketStream<TcpStream>, Message>,
    addr: &SocketAddr,
    token: &String,
    nonce: &mut String,
    timer: &Timer,
    db: &mut Connection,
    list_lock: Arc<Mutex<Vec<ConnectionInfo>>>
) -> Result<String, Error>{
    // println!("{}", msg);
    // println!("{} {}", msg.chars().take(5).collect::<String>(), msg.chars().take(5).collect::<String>() == "LOGIN");
    if msg.chars().take(6).collect::<String>() == "STATUS" {
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
                let _ = Ok::<String, String>("nonce updated".to_string());
            }
        } else {
            bail!("invalid status check for {}", addr);
        }
    } else if msg.chars().take(5).collect::<String>() == "LOGIN" {
        // println!("hi");
        // println!("{}", msg);
        let msg = msg.chars().skip(5).collect::<String>();
        // println!("{}", msg);
        let space_index = msg.find(" ").unwrap();
        let username = msg.chars().take(space_index).collect::<String>();
        let mut password = msg.chars().skip(space_index + 1).collect::<String>();
        println!("username: {}, password: {}", username, password);

        // use task::block in place for sql code to prevent errors
        let mut result = task::block_in_place(|| {
            let mut stmt = db.prepare("SELECT password, salt FROM Users WHERE username = ?1;").unwrap();
            let query_iter = stmt.query_map([username], |row| {
                Ok(password {
                    hash: row.get(0).unwrap(),
                    salt: row.get(1).unwrap(),
                })
            }).unwrap();
            return query_iter.collect::<Result<Vec<_>, _>>().unwrap();
        });

        let mut error = false;
        if !result.is_empty() {
            let password = &*password.into_bytes();
            let input_password = password.clone();
            let correct_password = result[0].hash.clone();
            let salt = result[0].salt.clone();
            println!("{}", SaltString::generate(&mut OsRng));
            let argon2 = Argon2::default();
            println!("argon2: {}", argon2.hash_password(password, SaltString::from_b64(&*salt).unwrap().as_salt()).unwrap().to_string());
            let password = PasswordHash::new(&*argon2.hash_password(password, SaltString::from_b64(&*salt).unwrap().as_salt()).unwrap().to_string()).unwrap();
            match argon2.verify_password(input_password, &PasswordHash::new(&*result[0].hash).unwrap()) {
                Ok(()) => println!("successfully verified password"),
                _ => {
                    error = true;
                    println!("failed to verify password");
                }
            }

        } else {
            error = true;
        }



        if error {
            if let Err(e) = sender.send(Message::from("1FAIL")).await {
                bail!("failed to send login error to {}: {}", addr, e);
            } else {
                info!("login error sent to {}", addr);
            }
        }

    } else {
        bail!("login screen received invalid message from {}: {}", addr, msg);
    }

    Ok("STATUS ok".to_string())
}