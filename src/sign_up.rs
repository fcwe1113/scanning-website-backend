use std::{future::Future, net::SocketAddr, sync::Arc};
use anyhow::{bail, Error};
use argon2::{Argon2, PasswordHasher, password_hash::{rand_core::OsRng, SaltString}};
use chrono::NaiveDateTime;
use futures_util::{SinkExt, stream::SplitSink};
use log::{debug, error, info};
use rusqlite::Connection;
use serde_email::is_valid_email;
use serde_json::Value;
use tokio::{net::TcpStream, sync::Mutex, task};
use tokio_rustls::server::TlsStream;
use tokio_tungstenite::WebSocketStream;
use tungstenite::Message;
use crate::{connection_info::ConnectionInfo, login_screen::token_status_check, screen_state::ScreenState};

#[derive(Clone)]
pub(crate) struct SignUpForm {
    username: String,
    password: String,
    first_name: String,
    last_name: String,
    dob_string: String,
    dob: NaiveDateTime,
    email: String,
}

impl SignUpForm {

    pub(crate) fn new_empty() -> Self {
        SignUpForm {
            username: String::new(),
            password: String::new(),
            first_name: String::new(),
            last_name: String::new(),
            dob_string: String::new(),
            dob: NaiveDateTime::MIN,
            email: String::new(),
        }
    }
    fn add_dob(&mut self, dob: &NaiveDateTime){
        self.dob = *dob;
    }

    fn remove_quotes(&mut self){
        self.username = self.username[1..self.username.len()-1].to_string();
        self.password = self.password[1..self.password.len()-1].to_string();
        self.first_name = self.first_name[1..self.first_name.len()-1].to_string();
        self.last_name = self.last_name[1..self.last_name.len()-1].to_string();
        self.email = self.email[1..self.email.len()-1].to_string();
    }
}

pub(crate) async fn sign_up_handler( // handler function for the start screen
                                     msg: &mut String,
                                     sender: &mut SplitSink<WebSocketStream<TlsStream<TcpStream>>, Message>,
                                     addr: &SocketAddr,
                                     token: &String,
                                     nonce: &mut String,
                                     session_username: &mut String,
                                     status_check_timer: &mut i32,
                                     list_lock: Arc<Mutex<Vec<ConnectionInfo>>>,
                                     sign_up_username_list_lock: Arc<Mutex<Vec<String>>>,
                                     sign_up_form: &mut SignUpForm,
                                     db: &mut Connection
) -> Result<(), Error>{

    // 2 = sign up screen
        // a. check items: token
        // b. do regular status checks until user either clicks sign up or cancel***
        // c. user enters all relevant info and client sanitises input***
        // d. client then sends the JSON form to backend***
        // with the format "2CHECK[JSON of the entire sign up form]"
        // e. server does another sanitize check
            // I. if there are errors then the list of error messages would be sent down prepended with "2BADFORM"
        // f. server run the username with db and a list of usernames being signed up
            // I. if no clashes from both, send "2NAMEOK" and put the name in the list
            // server should also send the verification email with the generated token (that is going to be ABCDEF) but nahhhhhhhhhhh
            // II. if a clash is found send "2BADNAME" and restart the process
        // g. client then does the email verification* and send back "2EMAILverify_token"***
            // i have a feeling that the token would be ABCDEF but idk
            // I. if the token is incorrect send "2BADEMAIL"
        // h. server runs the sql command to insert a new row containing all the given information, and send "2OK" after its done
        // i. move on to the store locator

    // debug!("Starting screen handler received {}", msg);
    // println!("{}", msg.chars().take(5).collect::<String>());
    // println!("{}", msg);
    let result = sign_up_screen(
        msg,
        sender,
        addr,
        token,
        nonce,
        status_check_timer,
        db,
        sign_up_form,
        session_username,
        sign_up_username_list_lock
    );
    if let Err(err) = resolve_result(result, addr, list_lock.clone()).await {
        bail!(err);
    }

    Ok(())

}

async fn sign_up_screen(
    msg: &mut String,
    sender: &mut SplitSink<WebSocketStream<TlsStream<TcpStream>>, Message>,
    addr: &SocketAddr,
    token: &String,
    nonce: &mut String,
    status_check_timer: &mut i32,
    db: &mut Connection,
    sign_up_form: &mut SignUpForm,
    session_username: &mut String,
    sign_up_username_list_lock: Arc<Mutex<Vec<String>>>
) -> Result<String, Error> {
    if msg.chars().take(6).collect::<String>() == "STATUS" {
        // messages starting with STATUS denotes that this is a regular status check
        let msg = msg.chars().skip(6).collect::<String>();
        match token_status_check(msg, token, addr, status_check_timer, nonce, sender).await{
            Ok(s) => { Ok(s) }
            Err(e) => { bail!(e) }
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
                if msg.chars().skip(5).collect::<String>() == *session_username {
                    if let Err(_) = sender.send(Message::from("2NEXT3")).await { // 2i.
                        bail!("failed to send moving on message to {}", addr);
                    }
                    Ok(Ok::<String, Error>("moving to store locator".to_string()).expect(""))
                } else {
                    bail!("invalid session username for client {}", addr);
                }
            }
            _ => {
                bail!("invalid login screen moving on code for {}", addr);
            }
        }
    } else if msg.chars().take(5).collect::<String>() == "CHECK" {
        let mut form = match serde_json::from_str::<Value>(msg.chars().skip(5).collect::<String>().as_str()) {
            Ok(json) => { SignUpForm {
                username: json["username"].to_string(),
                password: json["password"].to_string(),
                first_name: json["first_name"].to_string(),
                last_name: json["last_name"].to_string(),
                dob_string: json["dob"].to_string(),
                dob: NaiveDateTime::MIN, // just filling it with something to shut the compiler up
                email: json["email"].to_string(),
            } },
            Err(e) => { bail!("client {} sent invalid sign up form: {}", addr, e) }
        };
        form.remove_quotes();

        // sanitize inputs again bc who knows what can be in there
        let mut errors = String::new();
        if let Err(e) = sanitize(&mut form, &mut errors, addr) {
            bail!(e);
        }; // 2e. sanitising
        // println!("{}", form.dob.to_string());

        if !errors.is_empty() {
            sender.send(Message::from(format!("2BADFORM{}", errors))).await?; // 2eI.
            return Ok("STATUS ok".parse()?)
        }

        let result = task::block_in_place(|| { // 2f. check username against db
            let mut stmt = db.prepare("SELECT username FROM Users WHERE username = ?").unwrap();
            return stmt.query_map([&form.username], |row| {
                Ok(row.get(0)?)
            }).unwrap().collect::<Result<Vec<String>, _>>().unwrap()
        });
        if result.is_empty() { // if result vec is empty that means the username is not taken
            let mut used = false;
            for i in sign_up_username_list_lock.lock().await.iter() { // 2f. check username against usernames being signed up
                if *i == *form.username {
                    used = true;
                    break;
                }
            }

            if !used{
                sender.send(Message::from("2NAMEOK")).await?; // 2fI.
                debug!("client {} submitted an unused username: {}, please now pretend a confirmation email was sent to the submitted email of {}", addr, &form.username, &form.email);
                *sign_up_form = form.clone(); // save a copy of the form so we can fill in the sql insert query when the email was confirmed
                sign_up_username_list_lock.lock().await.push(form.username);
            } else {
                sender.send(Message::from("2BADNAME")).await?; // 2fII.
                debug!("client {} submitted a username in use: {}", addr, &form.username);
            }
        } else {
            sender.send(Message::from("2BADNAME")).await?; // 2fII.
            debug!("client {} submitted a username in use: {}", addr, &form.username);
        };

        Ok("STATUS ok".parse()?)
    } else if msg.chars().take(5).collect::<String>() == "EMAIL" {
        let verification_token = "ABCDEF"; // please pretend this line is taking in the generated token that was definitely sent to the new user's email
        let argon2 = Argon2::default();
        if msg.chars().skip(5).collect::<String>().as_str() == verification_token { // 2g. check against email verification token
            let _ = task::block_in_place(|| { // 2h. insert new user into db
                let tx = db.transaction().unwrap();
                tx.execute("INSERT INTO Users (username, password, first_name, last_name, dob, email) VALUES (?1, ?2, ?3, ?4, ?5, ?6)", [
                    sign_up_form.username.clone(),
                    argon2.hash_password(sign_up_form.password.clone().as_ref(), &SaltString::generate(&mut OsRng)).unwrap().to_string(),
                    sign_up_form.first_name.clone(),
                    sign_up_form.last_name.clone(),
                    sign_up_form.dob.clone().to_string(),
                    sign_up_form.email.clone()
                ]).unwrap();
                tx.commit().unwrap();
            });
            sender.send(Message::from("2OK")).await?; // 2h. tell the client account created
            debug!("client {} created a new user with username {}", addr, sign_up_form.username);
            *session_username = sign_up_form.username.clone();
            {
                let mut temp_vec = sign_up_username_list_lock.lock().await;
                let temp_temp_vec = temp_vec.clone();
                temp_vec.remove(temp_temp_vec.iter().position(|i| *i == sign_up_form.username).unwrap()); // remove the temp entry in the sign up list
            }

            *sign_up_form = SignUpForm::new_empty(); // delete the form to save memory

        } else {
            sender.send(Message::from("2BADEMAIL")).await?; // 2gI.
            debug!("client {} submitted incorrect email verification code", addr);
        }

        Ok("STATUS ok".parse()?)
    } else {
        bail!("sign up screen received invalid message from {}: {}", addr, msg);
    }
}

fn sanitize(form: &mut SignUpForm, errors: &mut String, addr: &SocketAddr) -> Result<(), Error> {
    // username (no spaces) (maybe block unicode?)
    if form.username.is_empty() || form.username == "\"\"" {
        bail!("client {} submitted empty username in sign up form, exiting", addr);
    } else if form.username.contains(|arg0: char| char::is_ascii_control(&arg0)) {
        bail!("client {} submitted control characters in sign up form username, exiting", addr);
    } else if form.username.contains(char::is_whitespace) {
        *errors += "username cannot contain whitespace characters\n";
    } else if form.username.contains(|c| String::from("\\{}[]:\"\'").chars().collect::<Vec<char>>().contains(&c)) {
        *errors += "username contains banned characters (\\{}[]:\"\')\n";
    }

    // password (need at least 1 upper and lower case char and a number, and at least 8 long, and limited to (non control)ascii)
    if form.password.is_empty() || form.password == "\"\"" {
        bail!("client {} submitted empty password in sign up form, exiting", addr);
    } else if !form.password.chars().all(|arg0: char| char::is_ascii(&arg0)) || form.password.contains(|arg0: char| char::is_ascii_control(&arg0)) {
        bail!("client {} submitted control characters in sign up form password, exiting", addr);
    } else if form.password.contains(|c| String::from("\\{}[]:\"\'").chars().collect::<Vec<char>>().contains(&c)) {
        *errors += "username contains banned characters (\\{}[]:\"\')\n";
    } else {
        if form.password.chars().count() < 8 {
            *errors += "password cannot be less than 8 characters\n";
        }
        if !form.password.contains(|arg0: char| char::is_ascii_lowercase(&arg0)) {
            *errors += "password does not have lower case characters\n";
        }
        if !form.password.contains(|arg0: char| char::is_ascii_uppercase(&arg0)) {
            *errors += "password does not have upper case characters\n";
        }
        if !form.password.contains(char::is_numeric) {
            *errors += "password does not have numeric characters\n";
        }
    }

    // maybe limit first/last name to ascii?
    if form.first_name.is_empty() || form.first_name == "\"\"" {
        bail!("client {} submitted empty first name in sign up form, exiting", addr);
    } else if form.first_name.contains(|arg0: char| char::is_ascii_control(&arg0)) {
        bail!("client {} submitted control characters in sign up form first name, exiting", addr);
    }

    if form.last_name.is_empty() || form.last_name == "\"\"" {
        bail!("client {} submitted empty last name in sign up form, exiting", addr);
    } else if form.last_name.contains(|arg0: char| char::is_ascii_control(&arg0)) {
        bail!("client {} submitted control characters in sign up form last name, exiting", addr);
    }

    // dob (following the format: YYYY-MM-DD)
    if form.dob_string.is_empty() || form.dob_string == "\"\"" {
        bail!("client {} submitted empty dob in sign up form, exiting", addr);
    } else {
        match NaiveDateTime::parse_from_str(&*format!("{} 00:00:00", form.dob_string), "\"%Y-%m-%d\" %H:%M:%S") {
            Ok(dob) => {
                form.add_dob(&dob);
            },
            Err(e) => {
                *errors += format!("failed to parse date of birth: {}\n", e).as_str();
            }
        }
    }

    // email ([chars]@[chars].[chars], all with non control ascii)
    if form.email.is_empty() || form.email == "\"\"" {
        bail!("client {} submitted empty email in sign up form, exiting", addr);
    } else if !form.email.chars().all(|arg0: char| char::is_ascii(&arg0)) || form.email.contains(|arg0: char| char::is_ascii_control(&arg0)) {
        bail!("client {} submitted control characters in sign up form email, exiting", addr);
    } else if !is_valid_email(form.email.clone()) {
        *errors += "email is not valid\n";
    }
    
    Ok(())
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