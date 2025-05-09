use std::{net::SocketAddr, sync::Arc};
use anyhow::{bail, Error};
use futures_util::{SinkExt, StreamExt};
use log::{debug, error, info};
use tokio::{net::TcpStream, sync::{Mutex, RwLock}, time::timeout};
use tokio_rustls::server::TlsStream;
use tokio_tungstenite::{accept_async, WebSocketStream};
use tungstenite::Message;
use rand::Rng;
use rand_chacha::{ChaCha20Rng, rand_core::SeedableRng};
use futures_util::stream::SplitSink;
use rusqlite::Connection;
use crate::{
    connection_info::ConnectionInfo,
    login_screen::start_screen_handler,
    main_app::{main_app_handler, CheckoutList},
    payment::payment_handler,
    sign_up::{sign_up_handler, SignUpForm},
    STATUS_CHECK_INTERVAL,
    APP_NONCE_LENGTH,
    store_locator::{store_locator_handler, ShopInfo},
    token_exchange::token_exchange_handler
};

// note:
// i tried to pass in the vector element reference but to no avail
// rust explicitly bans this unless ur willing to jump thru the hoops
// which im not
// for now every change to the list requires a mutex lock
pub(crate) async fn client_connection(
    stream: TlsStream<TcpStream>,
    addr: SocketAddr,
    token: String,
    mut token_exchanged: bool,
    mut nonce: String, // nonce will be 20 in length
    mut username: String,
    mut status_check_timer: i32,
    list_lock: Arc<Mutex<Vec<ConnectionInfo>>>,
    sign_up_username_list_lock: Arc<Mutex<Vec<String>>>,
    mut sign_up_form: SignUpForm,
    shop_list: Arc<RwLock<Vec<ShopInfo>>>,
    mut shop_id: i32,
    mut checkout_list: CheckoutList,
    mut db: Connection,
) {
    // note we dont want to lock the list and pass the list in by ref
    // do that and only one client can access the list until it dcs

    // this function handles the connection coming in
    // it first upgrades the connection from a normal request to a tcp channel
    // then it inits the receiver and sender so the server can talk to the client both ways

    // Accept the WebSocket connection
    let ws_stream = match accept_async(stream).await {
        Ok(ws) => {
            info!("New WebSocket connection made with {}", addr);
            ws
        },
        Err(e) => {
            error!("Error during the websocket handshake for address {}: {}", addr, e);
            return;
        }
    };

    // Split the WebSocket stream into a sender and receiver
    let (mut sender, mut receiver) = ws_stream.split();

    // send the token to the client
    if let Err(e) = sender.send(Message::from(format!("0{}", &token))).await /*0a sends public key to client*/ {
        error!("Error sending message to {}: {}", addr, e);
    } else {
        info!("token sent to {}: {}", addr, &token);
    }

    loop {
        match timeout(tokio::time::Duration::from_secs(5), receiver.next()).await { // set status check timer
            Ok(msg) => {
                // Handle incoming messages
                match msg.unwrap() {
                    Ok(Message::Text(text)) => {
                        debug!("raw Message received from {}: {}", addr, text);
                        // let text = String::from(text.as_str());
                        let text = match nonce_check(&nonce, &String::from(text.as_str()), &addr) {
                            Ok(s) => {s}
                            Err(e) => {
                                error!("{}", e);
                                return;
                            }
                        };

                        // first digit denotes client screen status
                        // after reading the first digit get rid of it and pass the rest of the message into the relevant function
                        // messages sent from both ends should follow a similar format (at least for the first few chars)

                        // status check would be done per set time period (2 minutes or something) confirming client status to prevent hacker fuckery
                        // each screen status (except 0) have a list of items used to do status checks
                        // in addition to that also a nonce which is a randomly generated string that every message sent up has to attach
                        // which is just the client pinging the server every set timeframe with the list of items
                        // if the server does not receive it in a set timeframe or the client's check items/nonce are wrong
                        // the server closes the connection and the client displays an error and stops functionality
                        // every status check also updates the clients nonce by replacing it with a new one by the server
                        // keep in mind that both client and server should have the same values and vars except for the one that they are actively changing

                        // *** denotes client side tasks
                        // 0 = token exchange
                        // 1 = start screen
                        // 2 = sign up screen
                        // 3 = store locator
                        // 4 = main app (the scanning screen)
                        // 5 = payment screen
                        // 6 = transferring to till (either by choice or to check id)
                        // 7 = after payment/logging out

                        let (first_char, msg) = match screen_check(&text, list_lock.clone(), &addr).await {
                            Ok((c, m)) => {(c, m)}
                            Err(e) => {
                                error!("{}", e);
                                return;
                            }
                        };

                        // println!("{}, {}", first_char, msg);
                        match first_char {
                            '0' => {

                                // error handling cant be packed into the function :(
                                if let Err(e) = token_exchange_handler(
                                    msg.clone(),
                                    &mut sender,
                                    &mut token_exchanged,
                                    &addr,
                                    &token,
                                    &mut nonce,
                                    list_lock.clone(),
                                ).await {
                                    // for now every error the server gets would lead to disconnect
                                    // maybe can implement a tier system later where some lead to retries
                                    // and others lead to straight disconnects

                                    error!("{}", e);
                                    break;
                                };

                                // let result = token_exchange(msg, &token, &mut sender, &addr, &token_exchanged);

                            }
                            '1' => {
                                if let Err(e) = start_screen_handler(
                                    &mut msg.clone(),
                                    &mut sender,
                                    &addr,
                                    &token,
                                    &mut nonce,
                                    &mut username,
                                    &mut status_check_timer,
                                    list_lock.clone(),
                                    &mut db,
                                ).await {
                                    error!("{}", e);
                                    break;
                                }
                            }
                            '2' => {
                                if let Err(e) = sign_up_handler(
                                    &mut msg.clone(),
                                    &mut sender,
                                    &addr,
                                    &token,
                                    &mut nonce,
                                    &mut username,
                                    &mut status_check_timer,
                                    list_lock.clone(),
                                    sign_up_username_list_lock.clone(),
                                    &mut sign_up_form,
                                    &mut db,
                                ).await {
                                    error!("{}", e);
                                    break;
                                }
                            }
                            '3' => {
                                if let Err(e) = store_locator_handler(
                                    &mut msg.clone(),
                                    &mut sender,
                                    &addr,
                                    &token,
                                    &mut nonce,
                                    &username,
                                    &mut status_check_timer,
                                    list_lock.clone(),
                                    shop_list.clone(),
                                    &mut shop_id,
                                ).await {
                                    error!("{}", e);
                                    break;
                                }
                            }
                            '4' => {
                                if let Err(e) = main_app_handler(
                                    &mut msg.clone(),
                                    &mut sender,
                                    &addr,
                                    &token,
                                    &mut nonce,
                                    &username,
                                    &mut status_check_timer,
                                    list_lock.clone(),
                                    &mut shop_id,
                                    &mut checkout_list,
                                    &mut db,
                                ).await {
                                    error!("{}", e);
                                    break;
                                }
                            }
                            '5' => {
                                if let Err(e) = payment_handler(
                                    &mut msg.clone(),
                                    &mut sender,
                                    &addr,
                                    &token,
                                    &mut nonce,
                                    &username,
                                    &mut status_check_timer,
                                    &mut shop_id,
                                    &mut checkout_list,
                                    &mut db,
                                ).await {
                                    error!("{}", e);
                                    break;
                                }
                            }
                            _ => { error!("lol") }
                        }
                    }
                    Ok(Message::Binary(_text)) => {
                        // binary strings are not supported and should not be sent from the front end anyways
                        // no to mention sending binary in react syntax is send("the string") compared to
                        // binary which is send(new Blob(["the string"]))

                        if let Err(e) = sender.send(Message::from("are u hacking me UWU")).await {
                            error!("Error sending message to {}: {}", addr, e);
                        } else {
                            info!("Sent to {}: are u hacking me UWU", addr);
                        }
                    }
                    Ok(Message::Close(_)) => {
                        let mut list = list_lock.lock().await;
                        for i in 0..list.len() - 1 {
                            if list[i].client_addr == addr {
                                list.remove(i);
                                info!("disconnected connection with {}", addr);
                                debug!("{:#?}", list);
                            }
                        }
                        break;
                    }
                    Ok(_) => (),
                    Err(e) => {
                        error!("Error processing message from {}: {}", addr, e);
                        break;
                    }
                };
            }
            Err(_) => {
                status_check_timer += 5;
                if status_check_timer >= STATUS_CHECK_INTERVAL {
                    error!("client {} failed to status check, exiting", addr);
                    break;
                }
            }
        };
    }
}

fn nonce_check(nonce: &String, msg: &String, addr: &SocketAddr) -> Result<String, Error> {
    if nonce == "-1" { 
        return Ok(msg.to_string());
    } else if msg.len() < APP_NONCE_LENGTH {
        bail!("client {} has incorrect screen state, exiting", addr)
    } else if msg.chars().take(APP_NONCE_LENGTH).collect::<String>() != *nonce {
        bail!("client {} has incorrect screen state, exiting", addr)
    }
    
    Ok(msg.to_string().chars().skip(APP_NONCE_LENGTH).collect())
}

async fn screen_check(text: &String, list_lock: Arc<Mutex<Vec<ConnectionInfo>>>, addr: &SocketAddr) -> Result<(char, String), Error> {
    // get first char
    let first_char = match text.chars().next() {
        Some(c) => c,
        None => {
            bail!("client {} has empty screen state, exiting", addr);
        },
    };

    // check if first char denotes the page client should be on
    for connection_info in list_lock.lock().await.iter() {
        if connection_info.client_addr == *addr {
            if char::to_digit(first_char, 10).unwrap() as i32 != connection_info.screen.as_i32() {
                bail!("client {} has incorrect screen state, exiting", addr);
            }
        }
    }
    
    Ok((first_char, text.chars().skip(1).collect()))
}

pub(crate) async fn update_nonce(nonce: &mut String, sender: &mut SplitSink<WebSocketStream<TlsStream<TcpStream>>, Message>, addr: &SocketAddr) -> Result<String, Error> {
    // generate a new nonce and send it over
    let mut rng = ChaCha20Rng::from_os_rng();
    *nonce = (0..APP_NONCE_LENGTH).map(|_| char::from(rng.random_range(32..127))).collect::<String>();
    if let Err(e) = sender.send(Message::from(format!("STATUS{}", nonce))).await {
        bail!("failed to send nonce to {}: {}", addr, e);
    } else {
        info!("updated nonce sent to {}: {}", addr, nonce);
        Ok("STATUS ok".to_string())
    }
}