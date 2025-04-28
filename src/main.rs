mod connection_info;
mod token_exchange;
mod screen_state;
mod client_connection;
mod login_screen;
mod sign_up;
mod store_locator;
mod main_app;
mod payment;

use crate::{
    client_connection::client_connection,
    connection_info::ConnectionInfo,
    token_exchange::*,
    main_app::CheckoutList,
    sign_up::SignUpForm,
    store_locator::ShopInfo
};
use anyhow::{bail, Error};
use log::{debug, error, info};
use rustls::{pki_types::{pem::PemObject, CertificateDer, PrivateKeyDer}};
use std::{
    net::SocketAddr,
    string::String,
    sync::Arc,
    time::Duration,
    fs,
    thread,
};
use chrono::Local;
use tokio::{net::TcpListener, sync::{Mutex, RwLock}, task};
use tokio_rustls::{rustls, TlsAcceptor};
use tracing_subscriber::{prelude::__tracing_subscriber_SubscriberExt, util::SubscriberInitExt};
use rusqlite::{Connection, Result};

// boilerplate is based on the example from https://github.com/campbellgoe/rust_websocket_server/blob/main/src/main.rs

// compress the folder to deploy to the server (change the ip if needed)
// then ssh into the server with this line "ssh -i "C:\Users\fcwe1113\Downloads\scanning-website-backend.pem" ubuntu@efrgtghyujhygrewds.ip-ddns.com" to run the backend
// the aws server will likely run out of memory while compiling the code so run "cargo run --release --jobs 1" to limit memory use while compiling

// the address to bind to aka which address the server listens to
// 0.0.0.0:8080 means to listen to everything coming into port 8080
const LISTENER_ADDR: &str = "0.0.0.0:8080";

// client should send a status check every 2 minutes, the 3 minutes here is to account of ping and other crap
const STATUS_CHECK_INTERVAL: i32 = 180; // in seconds

const DB_LOCATION: &str = "scanning_system.db";
const DB_BACKUP_LOCATION: &str = "scanning_system_backup.db";
const CERT_PATH: &str = "/etc/letsencrypt/live/efrgtghyujhygrewds.ip-ddns.com/fullchain.pem";
const PRIVATE_KEY_PATH: &str = "/etc/letsencrypt/live/efrgtghyujhygrewds.ip-ddns.com/privkey.pem";
const APP_NONCE_LENGTH: usize = 20;

// struct Test {
//     id: String,
//     name: String
// }

#[tokio::main]
async fn main() {

    // Initialize the logger
    tracing_subscriber::registry()
        .with(
            // for some reason to choose types of logs to allow past the filter cannot have spacebars so "info, debug" would not work
            // GREEEEEEAAAAAAATTTTTT
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "error,info,debug".into()).add_directive("mycrate".parse().unwrap()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // make the master list of all current active connections
    let connections_list: Vec<ConnectionInfo> = Vec::new();
    // slap a lock on that guy bc guy is popular and getting harassed by multiple ppl at once is bad
    let connections_list_lock = Arc::new(Mutex::new(connections_list));

    // make a list of usernames currently being registered
    let temp_sign_up_username_list: Vec<String> = Vec::new();
    let temp_sign_up_username_list_lock = Arc::new(Mutex::new(temp_sign_up_username_list)); // and mutex it

    // change the paths accordingly for server/local versions
    let cert = CertificateDer::from_pem_file(CERT_PATH).unwrap();
    let private_key = PrivateKeyDer::from_pem_file(PRIVATE_KEY_PATH).unwrap();
    debug!("TLS certificate loaded");

    // set up TLS acceptor
    // currently the cert is dealt with on the server side via certbot and letsencrypt
    // certbot: https://certbot.eff.org/
    // letsencrypt: https://letsencrypt.org/
    rustls::crypto::ring::default_provider().install_default().expect("Failed to install rustls crypto provider");
    let config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![CertificateDer::from(cert)], PrivateKeyDer::try_from(private_key).unwrap()).unwrap();

    // db testing
    // IMPORTANT!!!!!!!!!!!!!!!!!!!
    // REMEMBER TO USE "begin transaction" BEFORE CHANGING ANYTHING
    // OR ONE BAD COMMAND MEANS DEATH TO THE DBBBBBBBBBBBBBBBBB

    // this line will try and connect to a db and will cause a panic if it fails to connect to one
    let db = Connection::open(DB_LOCATION).unwrap();
    thread::spawn(|| { // thread to backup the db every 3 hrs
        loop{
            let _ = fs::copy(DB_LOCATION, DB_BACKUP_LOCATION); // yes it panics and crashes if it cant copy, no its not a bug its a feature
            info!("DB backed up on {}", Local::now());
            thread::sleep(Duration::from_secs(10800)); // thats 3 hours worth of seconds
        }
    });

    let shop_list: Arc<RwLock<Vec<ShopInfo>>> = Default::default();
    async fn shop_list_update(shop_list: Arc<RwLock<Vec<ShopInfo>>>) -> Result<(), Error> {
        let db = Connection::open(DB_LOCATION)?;
        loop{
            let temp_list = task::block_in_place(|| {
                let mut stmt = db.prepare("SELECT name, address FROM Shops").unwrap();
                return stmt.query_map([], |row| {
                    Ok(ShopInfo {
                        name: row.get(0).unwrap(),
                        address: row.get(1).unwrap()
                    })
                }).unwrap().collect::<Result<Vec<ShopInfo>>>().unwrap();
            });
            if temp_list.is_empty() {
                bail!("shop list is empty, indicating db is corrupted, exiting");
            }
            {
                let mut list = shop_list.write().await;
                *list = temp_list;
            }
            info!("shop list retrieved on {}", Local::now());
            tokio::time::sleep(Duration::from_secs(86400)).await; // thats 1 day worth of seconds
        }
    }
    // thread to update the shop list every day
    tokio::spawn(shop_list_update(shop_list.clone()));

    // let mut stmt = db.prepare("SELECT id, name FROM Test").unwrap(); // dont select * as the backend will need to anticipate rows to colect into lists
    // to receive select queries from the db the backend will need to prepare spots (aka vars) to store the incoming data
    // .query_map() is the executor of the command
    // below we used a self defined struct to store incoming data but theoretically cant u just add the strings together and decipher them the other end?
    // let mut stmt = db.prepare("SELECT id, name FROM Test").unwrap();
    // let query_iter = stmt.query_map([], |row| {
    //     Ok(Test {
    //         id: row.get(0).unwrap(),
    //         name: row.get(1).unwrap(),
    //     })
    // }).unwrap().collect::<Result<Vec<Test>>>().unwrap();
    //
    // for e in query_iter {
    //     let e = e;
    //     println!("{}|{}", e.id, e.name);
    // }

    let mut stmt = db.prepare("SELECT name FROM Test WHERE id = '01';").unwrap();
    let ans = stmt.query_map([], |row| {Ok(row.get(0).unwrap())}).unwrap().collect::<Result<Vec<String>>>().unwrap();
    let ans = &ans[0];
    // ? are holes you fill into the statement (prepared statements)
    db.execute("UPDATE Test SET name = ?1 WHERE id = '01';", [ans]).unwrap();

    // Create the TCP listener
    let listener = TcpListener::bind(&LISTENER_ADDR).await.expect("Failed to bind");

    info!("Listening on: {}", LISTENER_ADDR);

    let tls_acceptor = TlsAcceptor::from(Arc::new(config));

    // waits for an incoming connection and runs the loop if there is one coming in
    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                let incoming_addr = match stream.peer_addr().unwrap() { // get the client ip now because thats not possible after the connection is upgraded to TLS
                    SocketAddr::V4(v4addr) => {debug!("V4"); SocketAddr::from(v4addr)}
                    SocketAddr::V6(v6addr) => {debug!("V6"); SocketAddr::from(v6addr)}
                };
                let stream = match tls_acceptor.accept(stream).await {
                    Ok(stream) => {stream}
                    Err(e) => {error!("Error on tls handshake for client {}: {}", incoming_addr, e); break;}
                };

                let mut is_duplicate = false;

                // for some reason react intentionally opens the same websocket twice with one new websocket object init
                // we loop thru the connections list and if the ip is already on the list we drop the request
                // AAAANNNNDDDD it doesnt work as react connects in with different port for each connection
                // AAAANNNNDDDD it just uses the first connection and ignores the second connection memory leak style
                // todo(done)
                // either disable this dickish behaviour in react or somehow find a way to disconnect the redundant connection here
                // disabled it on react by disabling strict mode yaaaaaaaaaaaaaaayyyyyy
                // the duplicate check is kept in in case of other shenanigans that can happen with duplicate client ips
                // plus it shouldnt be happening in the first place anyways

                {
                    // this code here is surrounded with {} bc we want to ensure the entire code block here locks up
                    // the list and prevent anything else from interfering and escapes the duplicate check
                    // effectively this ensures one connection gets established before the next new client can start the
                    // connection process
                    let temp_connections_list = &mut connections_list_lock.lock().await;
                    for connections in temp_connections_list.iter() {
                        if connections.client_addr == incoming_addr {
                            is_duplicate = true;
                            break;
                        }
                    }

                    if !is_duplicate {
                        temp_connections_list.push(ConnectionInfo::new(incoming_addr, String::from("-1")));
                        info!("New connection from: {}", incoming_addr);
                        // debug!("{:#?}", temp_connections_list);
                        // Spawn a new task for each connection
                        // note this line makes a new thread for each connection
                        // 0a generate valid token
                        tokio::spawn(client_connection(
                            stream,
                            incoming_addr,
                            token_gen(&*temp_connections_list),
                            false,
                            String::from("-1"),
                            String::new(),
                            0,
                            connections_list_lock.clone(),
                            temp_sign_up_username_list_lock.clone(),
                            SignUpForm::new_empty(),
                            shop_list.clone(),
                            -1,
                            CheckoutList::new_empty(),
                            Connection::open(DB_LOCATION).unwrap(),
                        ));
                        // return Response::new(());
                    } else {
                        info!("duplicate connection request from: {}, dropping", incoming_addr);
                    }
                }
            },
            Err(e) => {
                error!("client TCP accept error: {}", e);
            }
        }
    }
}


