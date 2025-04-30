use std::{
    future::Future,
    net::SocketAddr,
    sync::Arc,
    ops::Add
};
use anyhow::{bail, Error};
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier, password_hash::{rand_core::OsRng, SaltString}};
use chrono::{Duration, Local, NaiveDateTime, NaiveTime};
use futures_util::{SinkExt, stream::SplitSink};
use log::{debug, error, info};
use rand::Rng;
use rand_chacha::{ChaCha20Rng, rand_core::SeedableRng};
use tokio::{net::TcpStream, sync::Mutex, task};
use tokio_tungstenite::WebSocketStream;
use tungstenite::Message;
use rusqlite::Connection;
use tokio_rustls::server::TlsStream;
use crate::{client_connection::update_nonce, connection_info::ConnectionInfo, screen_state::ScreenState};

pub(crate) async fn start_screen_handler( // handler function for the start screen
                                          msg: &mut String,
                                          sender: &mut SplitSink<WebSocketStream<TlsStream<TcpStream>>, Message>,
                                          addr: &SocketAddr,
                                          token: &String,
                                          nonce: &mut String,
                                          session_username: &mut String,
                                          status_check_timer: &mut i32,
                                          list_lock: Arc<Mutex<Vec<ConnectionInfo>>>,

                                          db: &mut Connection
) -> Result<(), Error>{

    // 1 = start screen (login)
        // a. check items: token
        // b. do regular status checks until user either clicks log in sign up or proceed as guest***
        // c. there are 3 branching paths of where the program can go (other than doing nothing)
            // A. user logins
                // 1. user fills in username and password and click login, client sends both username and password to server as "1LOGIN[username] [password]"***
                // 2. server queries db to get password of username
                // 3. if password is correct then server saves a local copy of the username and pings "1OK", otherwise it pings "1FAIL"
                // 4. client pings "1NEXT3[username]" to move to the store locator***
            // B. user signs up
                // 1. user clicks the sign up button and client pings "1NEXT2" to move to the sign up page***
            // C. user proceeds as guest
                // 1. user clicks the proceed as guest button and client pings "1GUEST"***
                // 2. server generates an unused guest name and pings that name down as "1GUEST[guest username]"
                // 3. client saves a copy and pings "1NEXT3[guest username]" to move to the store locator***

    // debug!("Starting screen handler received {}", msg);
    // println!("{}", msg.chars().take(5).collect::<String>());
    // println!("{}", msg);
    let result = start_screen(msg, sender, addr, &token, nonce, status_check_timer, db, session_username);
    if let Err(err) = resolve_result(result, addr, list_lock.clone()).await {
        bail!(err);
    }

    Ok(())
    
}

async fn start_screen(
    msg: &mut String,
    sender: &mut SplitSink<WebSocketStream<TlsStream<TcpStream>>, Message>,
    addr: &SocketAddr,
    token: &String,
    nonce: &mut String,
    status_check_timer: &mut i32,
    db: &mut Connection,
    session_username: &mut String) -> Result<String, Error>{
    // main start screen function dealing with incoming messages

    if msg.chars().take(6).collect::<String>() == "STATUS" { // 1b.
        // messages starting with STATUS denotes that this is a regular status check
        let msg = msg.chars().skip(6).collect::<String>();
        match token_status_check(msg, token, addr, status_check_timer, nonce, sender).await{
            Ok(s) => { Ok(s) }
            Err(e) => { bail!(e) }
        }
    } else if msg.chars().take(5).collect::<String>() == "LOGIN" {
        // messages starting with LOGIN denotes a login request
        // the submitted username and password should be after the LOGIN in the same message
        // in the format of "LOGIN[username][whitespace][password]" note usernames cannot have spaces
        let msg = msg.chars().skip(5).collect::<String>();
        if msg.len() < 3 {
            bail!("client {} sent invalid login request, exiting", addr);
        }
        let space_index = match msg.find(" ") {
            Some(i) => {
                if i == 0 {
                    bail!("client {} sent invalid login request, exiting", addr);
                }
                i
            },
            None => {
                bail!("client {} sent invalid login request, exiting", addr);
            }
        };
        let login_username = msg.chars().take(space_index).collect::<String>();
        let password = msg.chars().skip(space_index + 1).collect::<String>();

        // use task::block in place for sql code to prevent blocking other async tasks leading to deadlocks
        let result = task::block_in_place(|| { // 1cA1. querying db for password
            let mut stmt = db.prepare("SELECT password FROM Users WHERE username = ?1 AND TIME(dob) = \"00:00:00\";").unwrap();
            return stmt.query_map([login_username.clone()], |row| {
                Ok(row.get::<usize, String>(0).unwrap())
            }).unwrap().collect::<Result<Vec<_>, _>>().unwrap();
        });

        // error flag to show if login failed
        let mut error = false;
        if !result.is_empty() { // if the result vector is empty that means the given username is not in the db
            let password = &*password.into_bytes();
            match Argon2::default().verify_password(password, &PasswordHash::new(&*result[0])?) {
                Ok(()) => {
                    // do nothing
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
            sender.send(Message::from("1FAIL")).await?; // 1cA3. pings login fail message
            info!("login error sent to {}", addr);
            Ok("login fail".to_string())
        } else {
            sender.send(Message::from("1OK")).await?; // 1cA3. pings login success message
            info!("login success for {} as {}", addr, login_username);
            *session_username = login_username;
            Ok("login success".to_string())
        }

    } else if msg.chars().take(4).collect::<String>() == "NEXT" {
        // messages starting with NEXT means the client is moving onto another page
        let dest = msg.chars().skip(4).take(1).collect::<String>();
        match dest.as_str() {
            "2" => { // moving onto sign up page
                sender.send(Message::from("1NEXT2")).await?;
                Ok("moving to sign up".to_string())
            }
            "3" => { // moving onto store locator
                if msg.chars().skip(5).collect::<String>() == *session_username {
                    sender.send(Message::from("1NEXT3")).await?;
                    Ok("moving to store locator".to_string())
                } else {
                    bail!("invalid session username for client {} on login", addr);
                }
            }
            _ => {
                bail!("invalid login screen moving on code for {}", addr);
            }
        }
    } else if msg.chars().take(5).collect::<String>() == "GUEST" {
        // let mut guest_username;
        *session_username = {
            let mut rng = ChaCha20Rng::from_os_rng();
            let mut guest_username;
            loop {
                guest_username = String::from("Guest") + &*(0..10).map(|_| char::from(rng.random_range(32..127))).collect::<String>();
                let result = task::block_in_place(|| {
                    let mut stmt = db.prepare("SELECT username FROM Users WHERE username = ?1;").unwrap();
                    return stmt.query_map([guest_username.clone()], |row| {
                        Ok(row.get(0)?)
                    }).unwrap().collect::<Result<Vec<String>, _>>().unwrap();
                });
                if result.is_empty() {
                    break;
                }
            }
            let mut datetime = Local::now().naive_local();
            // since we treat user with 00:00:00 in their dob as real users
            // we need to force difference of somehow someone clicked on proceed as guest at 00:00:00
            if datetime.time() == NaiveTime::parse_from_str("00:00:00", "%H:%M:%S")? {
                datetime = NaiveDateTime::add(datetime, Duration::seconds(1));
            }
            let _ = task::block_in_place(|| { // 2h. insert new user into db
                let tx = db.transaction().unwrap();
                tx.execute("INSERT INTO Users (username, password, first_name, last_name, dob, email) VALUES (?1, ?2, ?3, ?4, ?5, ?6)", [
                    guest_username.clone(),
                    Argon2::default().hash_password("this is an amazing password".as_ref(), &SaltString::generate(&mut OsRng)).unwrap().to_string(),
                    String::from("Reese"),
                    String::from("Pineda"),
                    datetime.to_string(),
                    String::from("thisIs@nEmail.com")
                ]).unwrap();
                tx.commit().unwrap();
            });
            debug!("new guest created: {}", guest_username);
            sender.send(Message::from(format!("1GUEST{}", guest_username))).await?;
            guest_username
        };

        Ok(String::from("STATUS ok"))
    } else {
        bail!("login screen received invalid message from {}: {}", addr, msg);
    }
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

pub(crate) async fn token_status_check(msg: String, token: &String, addr: &SocketAddr, status_check_timer: &mut i32, nonce: &mut String, sender: &mut SplitSink<WebSocketStream<TlsStream<TcpStream>>, Message>) -> Result<String, Error> {
    if msg == *token {
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
}