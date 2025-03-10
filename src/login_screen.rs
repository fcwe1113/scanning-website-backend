use std::{
    future::Future,
    net::SocketAddr,
    sync::Arc
};
use anyhow::{bail, Error};
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier, password_hash::{rand_core::OsRng, SaltString}};
use chrono::Duration;
use futures_util::{SinkExt, stream::SplitSink};
use log::{debug, error, info};
use rand::{Rng};
use rand_chacha::{ChaCha20Rng, rand_core::SeedableRng};
use timer::Timer;
use tokio::{net::TcpStream, sync::Mutex, task};
use tokio_tungstenite::WebSocketStream;
use tungstenite::{Message, Utf8Bytes};
use rusqlite::{Connection, fallible_iterator::FallibleIterator};
use tokio_rustls::server::TlsStream;
use crate::connection_info::ConnectionInfo;
use crate::screen_state::ScreenState;
use crate::test;

struct password{ // essentially a container to hold the db query result
    hash: String,
}

pub(crate) async fn start_screen_handler( // handler function for the start screen
                                          msg: &mut String,
                                          sender: &mut SplitSink<WebSocketStream<TcpStream>, Message>,
                                          addr: &SocketAddr,
                                          token: &String,
                                          nonce: &mut String,
                                          session_username: &mut String,
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
    let result = start_screen(msg, sender, addr, &token, nonce, timer, db, session_username);
    if let Err(err) = resolve_result(result, addr, list_lock.clone()).await {
        bail!(err);
    }

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
    session_username: &mut String) -> Result<String, Error>{
    // main start screen function dealing with incoming messages

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
    } else if msg.chars().take(5).collect::<String>() == "LOGIN" {
        // messages starting with LOGIN denotes a login request
        // the submitted username and password should be after the LOGIN in the same message
        // in the format of "LOGIN[username][whitespace][password]" note usernames cannot have spaces
        let msg = msg.chars().skip(5).collect::<String>();
        let space_index = msg.find(" ").unwrap();
        let login_username = msg.chars().take(space_index).collect::<String>();
        let mut password = msg.chars().skip(space_index + 1).collect::<String>();
        // println!("username: {}, password: {}", login_username, password);

        // use task::block in place for sql code to prevent errors
        let mut result = task::block_in_place(|| {
            let mut stmt = db.prepare("SELECT password FROM Users WHERE username = ?1;").unwrap();
            let query_iter = stmt.query_map([login_username.clone()], |row| {
                Ok(password {
                    hash: row.get(0).unwrap(),
                })
            }).unwrap();
            return query_iter.collect::<Result<Vec<_>, _>>().unwrap();
        });

        // error flag to show if login failed
        let mut error = false;
        if !result.is_empty() { // if the result is empty that means the given username is not in the db
            let password = &*password.into_bytes();
            let argon2 = Argon2::default();
            match argon2.verify_password(password, &PasswordHash::new(&*result[0].hash).unwrap()) {
                Ok(()) => {
                    // do nothing
                    // debug!("{} logged in as {}", addr, login_username);
                }
                _ => {
                    error = true;
                    println!("failed to verify password");
                }
            }

        } else {
            error = true;
        }

        // if the error flag is true that means the login failed
        // otherwise it meant the login succeeded
        if error {
            if let Err(e) = sender.send(Message::from("1FAIL")).await {
                bail!("failed to send login error to {}: {}", addr, e);
            } else {
                info!("login error sent to {}", addr);
                Ok("login fail".to_string())
            }
        } else {
            if let Err(e) = sender.send(Message::from("1OK")).await {
                bail!("failed to send login sucess to {}: {}", addr, e);
            } else {
                info!("login success for {} as {}", addr, login_username);
                *session_username = login_username;
                Ok("login success".to_string())
            }
        }

    } else if msg.chars().take(4).collect::<String>() == "NEXT" {
        // messages starting with NEXT means the client is moving onto another page
        let dest = msg.chars().skip(4).take(1).collect::<String>();
        match dest.as_str() {
            "2" => { // moving onto sign up page
                if let Err(_) = sender.send(Message::from("1NEXT2")).await {
                    bail!("failed to send moving on message to {}", addr);
                }
                Ok(Ok::<String, Error>("moving to sign up".to_string()).expect(""))
            }
            "3" => { // moving onto store locator
                if msg.chars().skip(6).collect::<String>() == *session_username {
                    if let Err(_) = sender.send(Message::from("1NEXT3")).await {
                        bail!("failed to send moving on message to {}", addr);
                    }
                    Ok(Ok::<String, Error>("moving to store locator".to_string()).expect(""))
                } else {
                    bail!("invalid session username for client {} on login", addr);
                }
            }
            _ => {
                bail!("invalid login screen moving on code for {}", addr);
            }
        }
    } else {
        bail!("login screen received invalid message from {}: {}", addr, msg);
    }

    // Ok("STATUS ok".to_string())
}

async fn resolve_result(result: impl Future<Output=Result<String, Error>> + Sized, addr: &SocketAddr, list_lock: Arc<Mutex<Vec<ConnectionInfo>>>) -> Result<(), Error> {
    match result.await {
        Ok(r) => {
            // debug!("result: {}", r.as_str());
            match r.as_str() {

                "moving to sign up" => {
                    for connection_info in list_lock.lock().await.iter_mut() {
                        if connection_info.client_addr == *addr {
                            connection_info.screen = ScreenState::SignUp;
                        }
                    }
                    info!("moving client {} onto sign up screen", addr);
                    Ok(())
                },
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
                "login success" => {
                    Ok(()) // do nothing
                }
                "login fail" => {
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