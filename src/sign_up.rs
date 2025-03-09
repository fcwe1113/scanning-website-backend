use std::future::Future;
use std::net::SocketAddr;
use std::sync::Arc;
use anyhow::{bail, Error};
use chrono::{Duration, NaiveDateTime};
use futures_util::SinkExt;
use futures_util::stream::SplitSink;
use log::{debug, error, info};
use rand::Rng;
use rand_chacha::ChaCha20Rng;
use rand_chacha::rand_core::SeedableRng;
use rusqlite::Connection;
use serde_email::is_valid_email;
use serde_json::Value;
use timer::Timer;
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio_rustls::server::TlsStream;
use tokio_tungstenite::WebSocketStream;
use tungstenite::Message;
use crate::connection_info::ConnectionInfo;
use crate::screen_state::ScreenState;

struct sign_up_form{
    username: String,
    password: String,
    first_name: String,
    last_name: String,
    dob_string: String,
    dob: NaiveDateTime,
    email: String,
}

impl sign_up_form{
    fn add_dob(&mut self, dob: &NaiveDateTime){
        self.dob = *dob;
    }
}

pub(crate) async fn sign_up_handler( // handler function for the start screen
                                     msg: &mut String,
                                     sender: &mut SplitSink<WebSocketStream<TcpStream>, Message>,
                                     addr: &SocketAddr,
                                     token: &String,
                                     nonce: &mut String,
                                     username: &mut String,
                                     timer: &Timer,
                                     list_lock: Arc<Mutex<Vec<ConnectionInfo>>>,
                                     sign_up_username_list_lock: Arc<Mutex<Vec<String>>>,
                                     db: &mut Connection
) -> Result<(), Error>{

    // 2 = sign up screen
        // a. check items: token
        // b. do regular status checks until user either clicks log in sign up or proceed as guest***
        // c. if user enters all relevant info and client sanitises input***
        // d. client then sends username to backend to check for clashes***
        // with the format "2CHECK [JSON of the entire sign up form]"
        // e. server does another sanitize check and run the username with db and a list of usernames being signed up
            // I. if no clashes from both, send "2NAMEOK" and put the name in the list
            // server should also send the verification email with the generated token (that is going to be ABCDEF) but nahhhhhhhhhhh
            // II. if a clash is found send "2BADNAME" and restart the process
        // f. client then does the email verification* and send back "2EMAIL verify_token"***
        // i have a feeling that the token would be ABCDEF but idk
            // I. if the token is incorrect send "2BADEMAIL"
        // g. server runs the sql command to insert a new row containing all the given information, and send "2OK" after its done
        // h. move on to the store locator

    // debug!("Starting screen handler received {}", msg);
    // println!("{}", msg.chars().take(5).collect::<String>());
    // println!("{}", msg);
    let result = sign_up_screen(msg, sender, addr, token, nonce, timer, db, username);
    if let Err(err) = resolve_result(result, addr, list_lock.clone()).await {
        bail!(err);
    }

    Ok(())

}

async fn sign_up_screen(
    msg: &mut String,
    sender: &mut SplitSink<WebSocketStream<TcpStream>, Message>,
    addr: &SocketAddr,
    token: &String,
    nonce: &mut String,
    timer: &Timer,
    db: &mut Connection,
    username: &mut String
) -> Result<String, Error> {
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
    } else if msg.chars().take(4).collect::<String>() == "NEXT" {
        // messages starting with NEXT means the client is moving onto another page
        let dest = msg.chars().skip(4).take(1).collect::<String>();
        match dest.as_str() {
            "1" => { // moving onto login page
                if let Err(_) = sender.send(Message::from("2NEXT1")).await {
                    bail!("failed to send moving on message to {}", addr);
                }
                Ok(Ok::<String, Error>("moving to login".to_string()).expect(""))
            }
            "3" => { // moving onto store locator
                if let Err(_) = sender.send(Message::from("2NEXT3")).await {
                    bail!("failed to send moving on message to {}", addr);
                }
                Ok(Ok::<String, Error>("moving to store locator".to_string()).expect(""))
            }
            _ => {
                bail!("invalid login screen moving on code for {}", addr);
            }
        }
    } else if msg.chars().take(5).collect::<String>() == "CHECK" {
        let json: Value = serde_json::from_str(msg.chars().skip(6).collect::<String>().as_str())?;
        let mut form = sign_up_form{
            username: json["username"].to_string(),
            password: json["password"].to_string(),
            first_name: json["first_name"].to_string(),
            last_name: json["last_name"].to_string(),
            dob_string: json["dob_string"].to_string(),
            dob: NaiveDateTime::MIN, // just filling it with something to shut it up
            email: json["email"].to_string(),
        };

        // sanitize inputs again bc who knows what can be in there
        let mut errors = String::new();
        // username (no spaces)
        if form.username == "" {
            errors += "username is empty\n";
        } else if form.username.contains(char::is_whitespace) {
            errors += "username cannot contain whitespace characters\n";
        }

        // password (need at least 1 upper and lower case char and a number, and at least 8 long, and limited to (non control)ascii)
        if form.password == "" {
            errors += "password is empty\n";
        } else if form.password.chars().all(char::is_ascii) && !form.password.contains(char::is_ascii_control) {
            errors += "password can contain only (non control) ASCII characters\n";
        } else {
            if form.password.len() < 8 {
                errors += "password cannot be less than 8 characters\n";
            }
            if form.password.contains(char::is_ascii_lowercase) {
                errors += "password does not have lower case characters\n";
            }
            if form.password.contains(char::is_ascii_uppercase) {
                errors += "password does not have upper case characters\n";
            }
            if form.password.contains(char::is_numeric) {
                errors += "password does not have numeric characters\n";
            }
        }

        // maybe limit first/last name to ascii?
        if form.first_name == "" {
            errors += "first name is empty\n";
        }
        if form.last_name == "" {
            errors += "last name is empty\n";
        }

        // dob (following the format: YYYY-MM-DD)
        if form.dob_string == "" {
            errors += "date of birth is empty\n";
        } else {
            match NaiveDateTime::parse_from_str(&*format!("{} 00:00:00", form.dob_string), "%Y-%m-%d %H:%M:%S") {
                Ok(dob) => {
                    form.add_dob(&dob);
                },
                Err(e) => {
                    errors += format!("failed to parse date of birth: {}\n", e).as_str();
                }
            }
        }

        // email ([chars]@[chars].[chars], all with non control ascii)
        if form.email == "" {
            errors += "email is empty\n";
        } else if form.email.chars().all(char::is_ascii) && !form.email.contains(char::is_ascii_control) {
            errors += "email can contain only (non control) ASCII characters\n";
        } else if !is_valid_email(form.email) {
            errors += "email is not valid\n";
        }

        Ok("dsfbs".parse()?)
    } else if msg.chars().take(5).collect::<String>() == "EMAIL" {


        Ok("dsfbs".parse()?)
    } else {
        bail!("sign up screen received invalid message from {}: {}", addr, msg);
    }
}

async fn resolve_result(result: impl Future<Output=Result<String, Error>> + Sized, addr: &SocketAddr, list_lock: Arc<Mutex<Vec<ConnectionInfo>>>) -> Result<(), Error> {
    match result.await {
        Ok(r) => {
            // debug!("result: {}", r.as_str());
            match r.as_str() {

                "moving to store locator" => {
                    for connection_info in list_lock.lock().await.iter_mut() {
                        if connection_info.client_addr == *addr {
                            connection_info.screen = ScreenState::StoreLocator;
                        }
                    }
                    info!("moving client {} onto store locator screen", addr);
                    Ok(())
                },
                "moving to login" => {
                    for connection_info in list_lock.lock().await.iter_mut() {
                        if connection_info.client_addr == *addr {
                            connection_info.screen = ScreenState::Start;
                        }
                    }
                    info!("moving client {} onto store locator screen", addr);
                    Ok(())
                }
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