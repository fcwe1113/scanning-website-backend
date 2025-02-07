use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use futures_util::{SinkExt, StreamExt};
use log::{debug, error, info};
use tokio::net::TcpStream;
use tokio_tungstenite::accept_async;
use tungstenite::{Message, Utf8Bytes};
use crate::connection_info::ConnectionInfo;
use crate::screen_state::ScreenState;
use crate::token_exchange::token_exchange;

// note:
// i tried to pass in the vector element reference but to no avail
// rust explicitly bans this unless ur willing to jump thru the hoops
// which im not
// for now every change to the list requires a mutex lock
pub(crate) async fn client_connection(stream: TcpStream, addr: SocketAddr, token: String, mut token_exchanged: bool, list_lock: Arc<Mutex<Vec<ConnectionInfo>>>) {
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
    if let Err(e) = sender.send(Message::Text(format!("0{}",token).into())).await /*0a sends token to client*/ {
        error!("Error sending message to {}: {}", addr, e);
    } else {
        info!("Token sent to {}: {}", addr, token);
    }

    // Handle incoming messages
    while let Some(msg) = receiver.next().await {
        match msg {
            Ok(Message::Text(text)) => {
                debug!("Message received from {}: {}", addr, text);

                // ideally a status check should be done per set time period confirming client status to prevent hacker fuckery
                // and both sides should move in lock step anyways to prevent bugs
                // keep in mind that both client and server should have the same values and vars except for the one that they are actively changing

                // first digit denotes client screen status
                // after reading the first digit get rid of it and pass the rest of the message into the relevant function
                // messages sent from both ends should follow a similar format (at least for the first few chars)
                // *** denotes client side tasks
                // 0 = token exchange
                // a. when the websocket channel opens the server will generate the token and send it to the client, the client would have a placeholder token which prevent the client from proceeding
                // b. the client saves the token and pings back the same token to the server***
                // c. the server sends an ack back if the token matches and switches the token_exchanged flag on and saves it in the list(tm)
                // d. client then tells the server to move on to the start screen state***
                // e. server tells client to move on then moves on itself, unless the token_exchanged flag is not on, in which case handle the error
                // f. client moves on for real***
                // while the client is waiting for the token exchange ack it can show a loading wheel or something idk
                // 1 = start screen
                // 2 = sign up screen
                // 3 = store locator
                // 4 = main app (the scanning screen)
                // 5 = payment screen
                // 6 = transferring to till (either by choice or to check id)
                // 7 = after payment/logging out

                // vars indicating client status split by which relevant stage the client is in
                // token exchange
                //let mut token_exchanged = false; // used in step 0e

                // get first char
                let first_char = text.chars().next().unwrap();

                // get the rest of the string
                let msg = text.chars().next().map(|c| &text[c.len_utf8()..]).unwrap().to_string(); // 0a
                // debug!("msg: {} {}", msg, token);
                // no need to lock anything used here as no message that can interfere with each other should interfere with each other
                match first_char {
                    '0' => {
                        let result = token_exchange(msg, &token, &mut sender, &addr, &token_exchanged);
                        match result.await {
                            Ok(r) => {
                                match r.as_str() {
                                    "token ackked" => {
                                        debug!("Token ackked");
                                        token_exchanged = true; // 0c flag
                                        debug!("{:#?}", token_exchanged);
                                        for Connection in list_lock.lock().unwrap().iter_mut() {
                                            if Connection.addr == addr {
                                                Connection.token = token.clone(); // 0c saves on list
                                            }
                                        };
                                    }, //0c
                                    "moving on" => {
                                        for connection_info in list_lock.lock().unwrap().iter_mut() {
                                            if connection_info.addr == addr {
                                                connection_info.screen = ScreenState::Start;
                                            }
                                        }
                                        println!("moving onto start screen");

                                    }, // 0e moving on todo
                                    _ => error!("how did this happen lol")
                                }
                            },
                            Err(e) => {
                                // for now every error the server gets would lead to disconnect
                                // maybe can implement a tier system later where some lead to retries
                                // and others lead to straight disconnects
                                error!("{}", e);
                                break;
                            }
                        }
                    },
                    '1' => info!("passing \"{}\" into start screen func", msg),
                    _ => { error!("lol") }
                }


                // Reverse the received string and send it back
                info!("Received from {}: {}", addr, text);
                let reversed = text.chars().rev().collect::<String>();
                if let Err(e) = sender.send(Message::Text(reversed.clone().into())).await {
                    error!("Error sending message to {}: {}", addr, e);
                } else {
                    info!("Sent to {}: {}", addr, reversed);
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
                    if list[i].addr == addr {
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