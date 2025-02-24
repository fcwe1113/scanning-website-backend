use std::future::Future;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use anyhow::{bail, Error};
use futures_util::{SinkExt, StreamExt};
use log::{debug, error, info};
use openssl::encrypt::Decrypter;
use openssl::pkey::{PKey, Private};
use openssl::rsa::{Padding, Rsa};
use tokio::net::TcpStream;
use tokio_rustls::server::TlsStream;
use tokio_tungstenite::accept_async;
use tungstenite::{Message, Utf8Bytes};
use base64::decode;
use openssl::aes::AesKey;
use openssl::hash::MessageDigest;
use rand::Rng;
use rand_chacha::ChaCha20Rng;
use rand_chacha::rand_core::SeedableRng;
use ring::aead::quic::AES_256;
use chrono::{DateTime, Duration, Utc};
use rusqlite::Connection;
use timer::Timer;
use crate::connection_info::ConnectionInfo;
use crate::login_screen::start_screen_handler;
use crate::screen_state::ScreenState;
use crate::token_exchange::token_exchange_handler;

// note:
// i tried to pass in the vector element reference but to no avail
// rust explicitly bans this unless ur willing to jump thru the hoops
// which im not
// for now every change to the list requires a mutex lock
pub(crate) async fn client_connection(
    stream: TcpStream,
    addr: SocketAddr,
    token: String,
    mut token_exchanged: bool,
    mut nonce: String, // nonce will be 20 in length
    mut last_check: DateTime<Utc>,
    timer: Timer,
    list_lock: Arc<Mutex<Vec<ConnectionInfo>>>,
    mut db: Connection
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

    // 0a. generating key pair
    // todo try to use https://github.com/ravikiran232/rsa to generate keys in OAEP encoding as the client code only supports that
    // use https://anycript.com/crypto/rsa to double check

    // let aes = AesKey::new_encrypt(&(0..32).map(|_| u8::try_from(rng.random_range(0..255)).unwrap()).collect::<Vec<u8>>()).unwrap();
    // let rsa = Rsa::generate(2048).unwrap();
    // let key = PKey::from_rsa(rsa.clone()).unwrap(); // contains both the public key and private key
    // println!("{}", String::from_utf8(key.private_key_to_pem_pkcs8().unwrap()).unwrap());
    // let client_private_key: String;
    // let mut decrypter = Decrypter::new(&key).unwrap();
    // decrypter.set_rsa_padding(Padding::NONE).unwrap();

    // println!("public key {:?}", public_key);

    // Split the WebSocket stream into a sender and receiver
    let (mut sender, mut receiver) = ws_stream.split();

    // send the token to the client
    if let Err(e) = sender.send(Message::from(format!("0{}", token.clone()))).await /*0a sends public key to client*/ {
        error!("Error sending message to {}: {}", addr, e);
    } else {
        info!("token sent to {}: {}", addr, &token);
    }

    // set status check timer
    // println!("timer set");
    timer.schedule_with_delay(Duration::minutes(3), move || {return;});

    // Handle incoming messages
    while let Some(msg) = receiver.next().await {
        match msg {
            Ok(Message::Text(text)) => {
                debug!("raw Message received from {}: {}", addr, text);
                let mut text = String::from(text.as_str());

                if nonce.as_str() != "-1" {
                    if text[..20] != nonce {
                        error!("client {} has invalid nonce: {}", addr, text);
                        break;
                    } else {
                        text = text.replace(nonce.clone().as_str(), "");
                    }
                }

                // encrypting messages are getting benched right now
                // let text = text.as_bytes();
                //
                // // Decrypt the data
                // let mut decrypter = Decrypter::new(&key).unwrap();
                // decrypter.set_rsa_padding(Padding::NONE).unwrap();
                // decrypter.set_rsa_oaep_md(MessageDigest::sha512()).expect("wdym cant set oaep md");
                // // Create an output buffer
                // let buffer_len = decrypter.decrypt_len(&text).unwrap();
                // let mut decrypted = vec![0; buffer_len];
                // // Encrypt and truncate the buffer
                // let decrypted_len = decrypter.decrypt(&text, &mut decrypted).unwrap();
                // decrypted.truncate(decrypted_len);
                // println!("{}", String::from_utf8(decrypted).unwrap());

                // let data_len = text.len();
                // let buf_len = rsa.size() as usize;
                // println!("buf {} data {}", buf_len, data_len);
                // let mut buffer: Vec<u8> = vec![0; buf_len];
                // let mut decrypted_data: Vec<u8> = Vec::new();
                // println!("{}", rsa.size());
                // for chunk in text.chunks(buf_len) {
                //     rsa.private_decrypt(chunk, &mut buffer, Padding::NONE).expect("Error Decrypting");;
                //     decrypted_data.extend_from_slice(buffer.as_slice());
                // }
                // println!("{:?}", String::from_utf8(decrypted_data));


                // let text = "lol";
                // debug!("Message received from {}: {}", addr, text);


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
                // 2 = sign up screen
                    // NOTE:
                // 3 = store locator
                // 4 = main app (the scanning screen)
                // 5 = payment screen
                // 6 = transferring to till (either by choice or to check id)
                // 7 = after payment/logging out

                // get first char
                let first_char = text.chars().next().unwrap();

                // get the rest of the string
                let msg = text.chars().next().map(|c| &text[c.len_utf8()..]).unwrap().to_string();
                // no need to lock anything used here as no message that can interfere with each other should interfere with each other

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
                            list_lock.clone()
                        ).await{
                            // for now every error the server gets would lead to disconnect
                            // maybe can implement a tier system later where some lead to retries
                            // and others lead to straight disconnects

                            error!("{}", e);
                            break;
                        };

                        // let result = token_exchange(msg, &token, &mut sender, &addr, &token_exchanged);

                    },
                    '1' => { if let Err(e) = start_screen_handler(
                        &mut msg.clone(),
                        &mut sender,
                        &addr,
                        &token,
                        &mut nonce,
                        &timer,
                        list_lock.clone(),
                        &mut db
                    ).await{
                        error!("{}", e);
                        break;
                    } },
                    _ => { error!("lol") }
                }

            }
            Ok(Message::Binary(_text)) => {
                // binary strings are not supported and should not be sent from the front end anyways
                // no to mention sending binary in react syntax is send("the string") compared to
                // binary which is send(new Blob(["the string"]))

                if let Err(e) = sender.send(Message::Text(Utf8Bytes::from("are u hacking me UWU"))).await {
                    error!("Error sending message to {}: {}", addr, e);
                } else {
                    info!("Sent to {}: are u hacking me UWU", addr);
                }
            }
            Ok(Message::Close(_)) => {
                let mut list = list_lock.lock().unwrap();
                for i in 0..list.len() - 1 {
                    if list[i].client_addr == addr {
                        list.remove(i);
                        info!("disconnected connection with {}", addr);
                        debug!("{:#?}", list);
                    }
                }
                break
            },
            Ok(_) => (),
            Err(e) => {
                error!("Error processing message from {}: {}", addr, e);
                break;
            }
        }
    }
}