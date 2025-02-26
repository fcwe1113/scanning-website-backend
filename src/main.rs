mod connection_info;
mod token_exchange;
mod screen_state;
mod client_connection;
mod tls_cert_gen;
mod login_screen;

use crate::client_connection::client_connection;
use crate::connection_info::ConnectionInfo;
use crate::screen_state::ScreenState;
use crate::tls_cert_gen::{generate_acme_cert, generate_self_signed_cert};
use crate::token_exchange::*;
use acme2::{AccountBuilder, DirectoryBuilder, OrderBuilder};
use anyhow::Context;
use axum::{
    http::{Response, StatusCode},
    response::IntoResponse,
    routing::{any, get, post},
    debug_handler,
    Router,
    ServiceExt,
    handler::Handler
};
use axum_server::{Server, tls_rustls::RustlsConfig};
use clap::builder::Str;
use futures::{SinkExt, StreamExt};
use log::{debug, error, info};
use rand::{
    distr::Alphanumeric,
    rng,
    Rng,
    RngCore,
    SeedableRng};
use rand_chacha::ChaCha20Rng;
use rustls::{
    pki_types::{pem::PemObject, CertificateDer, PrivateKeyDer},
    crypto::CryptoProvider
};
use std::{env, iter, net::SocketAddr, string::String, sync::Arc, time::Duration, collections::HashMap, fs, thread, time};
use chrono::Utc;
use futures_util::task::SpawnExt;
use timer::Timer;
use tokio::{
    net::{TcpListener, TcpStream},
    sync::Mutex
};
use tokio_tungstenite::{accept_async, tungstenite::protocol::Message};
use tokio_rustls::{rustls, TlsAcceptor};
use tokio_rustls_acme::acme::ChallengeType;
use tracing_subscriber::{prelude::__tracing_subscriber_SubscriberExt, util::SubscriberInitExt};
use tungstenite::Utf8Bytes;
use unicode_segmentation::UnicodeSegmentation;
use warp::Filter;
use rusqlite::{Connection, Result};

// boilerplate is based on the example from https://github.com/campbellgoe/rust_websocket_server/blob/main/src/main.rs

// Get the address to bind to aka which address the server listens to
const LISTENER_ADDR: &str = "0.0.0.0:8080";

// client should send a status check every 2 minutes, the 3 minutes here is to account of ping and other crap
const STATUS_CHECK_INTERVAL: Duration = Duration::from_secs(180);

const DB_LOCATION: &str = "C:\\Users\\fcwe1113\\Downloads\\sqlite-tools-win-x64-202501281250\\scanning_system.db";
const DB_BACKUP_LOCATION: &str = "C:\\Users\\fcwe1113\\Downloads\\sqlite-tools-win-x64-202501281250\\scanning_system_backup.db";

struct test {
    id: String,
    name: String
}

#[tokio::main]
async fn main() {

    // let mut rng = ChaCha20Rng::from_os_rng();
    //
    // for i in 0..10 {
    //     // salt generator for account creation
    //     println!("{}", (0..20).map(|_| char::from(rng.random_range(32..127))).collect::<String>());
    // }

    // Initialize the logger
    tracing_subscriber::registry()
        .with(
            // for some reason to choose types of logs to allow past the filter cannot have spacebars so "info, debug" would not work
            // GREEEEEEAAAAAAATTTTTT
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "error,info,debug".into()).add_directive("mycrate".parse().unwrap()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // let current_dir = std::env::current_dir().expect("failed to read current directory");
    // debug!("current directory: {:?}", current_dir);
    // let routes = warp::get().and(warp::fs::dir(current_dir));warp::serve(routes)
    //     .tls()
    //     .cert_path("cert.pem")
    //     .key_path("key.rsa")
    //     .run(([0, 0, 0, 0], 9231)).await;

    // make the master list of all current active connections
    let mut connections_list: Vec<ConnectionInfo> = Vec::new();
    // slap a lock on that guy bc guy is popular and getting harassed by multiple ppl at once is bad
    let connections_list_lock = Arc::new(Mutex::new(connections_list));

    // EVERYTHING FROM HERE IS SKIPPED
    // EVERYTHING FROM HERE IS SKIPPED
    // EVERYTHING FROM HERE IS SKIPPED

    // generate the cert and the private key
    // let (b) = generate_acme_cert().await.unwrap();

    // let (mut cert, private_key) = generate_self_signed_cert().unwrap();
    // let cert = cert.to_der().unwrap();
    // let private_key = private_key.private_key_to_der().unwrap();

    let cert = CertificateDer::from_pem_file("localhost.crt").unwrap();
    let private_key = PrivateKeyDer::from_pem_file("localhost.key").unwrap();

    // set up TLS acceptor
    // currently the cert we self made was not trusted by the client and bc we didnt handle the error here the server dies
    // https://letsencrypt.org/getting-started/ provides free valid certs
    // and https://docs.rs/acme2/latest/acme2/ provides the way to get that cert in program
    // todo use that instead of the self made one
    // we can still keep it as a plan b if the valid cert is unavailable somehow
    rustls::crypto::ring::default_provider().install_default().expect("Failed to install rustls crypto provider");
    let config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![CertificateDer::from(cert)], PrivateKeyDer::try_from(private_key).unwrap()).unwrap();
    let acceptor = TlsAcceptor::from(Arc::new(config));

    // SKIPPING SECTION END
    // SKIPPING SECTION END
    // SKIPPING SECTION END

    // db testing
    // IMPORTANT!!!!!!!!!!!!!!!!!!!
    // REMEMBER TO USE "begin transaction" BEFORE CHANGING ANYTHING
    // OR ONE BAD COMMAND MEANS DEATH TO THE DBBBBBBBBBBBBBBBBB

    // make a backup when the server first start
    // let mut backup_timer = Timer::new();
    // db_backup(&mut backup_timer);
    // let backup_scheduler = thread::spawn(|| {
    //
    // });

    // this line will try and connect to a db and will cause a panic if it fails to connect to one
    let mut db = Connection::open(DB_LOCATION).unwrap();
    thread::spawn(|| {
        loop{
            fs::copy(DB_LOCATION, DB_BACKUP_LOCATION); // yes it panics and crashes if it cant copy, no its not a bug its a feature
            info!("DB backed up");
            thread::sleep(Duration::from_secs(10800)); // thats 3 hours worth of seconds
        }
    });


    let mut stmt = db.prepare("SELECT id, name FROM test").unwrap(); // dont select * as the backend will need to anticipate rows to colect into lists
    // to receive select queries from the db the backend will need to prepare spots (aka vars) to store the incoming data
    // .query_map() is the executor of the command
    // below we used a self defined struct to store incoming data but theoretically cant u just add the strings together and decipher them the other end?
    let query_iter = stmt.query_map([], |row| {
        Ok(test {
            id: row.get(0).unwrap(),
            name: row.get(1).unwrap(),
        })
    }).unwrap();

    for e in query_iter {
        let e = e.unwrap();
        println!("{}|{}", e.id, e.name);
    }

    let mut ans = db.prepare("SELECT name FROM test WHERE id = '01';").unwrap();
    let ans = ans.query_map([], |row| {Ok(row.get(0).unwrap())}).unwrap();
    let mut output = String::new();
    for a in ans {
        output = a.unwrap();
    }
    println!("{}", output);
    output += &*String::from(output.chars().last().unwrap());
    // ? are holes you fill into the statement
    db.execute("UPDATE test SET name = ?1 WHERE id = '01';", [output]).unwrap();

    // Create the TCP listener
    let listener = TcpListener::bind(&LISTENER_ADDR).await.expect("Failed to bind");

    info!("Listening on: {}", LISTENER_ADDR);

    // waits for an incoming connection and runs the loop if there is one coming in
    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                let incoming_addr = stream.peer_addr().unwrap();

                // let acceptor = acceptor.clone();
                // let mut tls_stream = acceptor.accept(stream).await.unwrap();

                // NOTE:
                // this TLS adventure is a dead end as to even attempt to make a valid cert
                // you need a domain name for the backend which i dont and very likely wont have ever
                // source: https://users.rust-lang.org/t/ed25519-and-rustls-tls-client-server/80133/4
                // at this current stage there is no point in continuing this further

                let mut is_duplicate = false;

                // for some reason react intentionally opens the same websocket twice with one new websocket object init
                // we loop thru the connections list and if the ip is already on the list we drop the request
                // AAAANNNNDDDD it doesnt work as react connects in with different port for each connection
                // AAAANNNNDDDD it just uses the first connection and ignores the second connection memory leak style
                // todo0
                // either disable this dickish behaviour in react or somehow find a way to disconnect the redundant connection here
                // disabled it on react by disabling strict mode yaaaaaaaaaaaaaaayyyyyy

                {
                    // this code here is surrounded with {} bc we want to ensure the entire code block here locks up
                    // the list and prevent anything else from interfering and escapes the duplicate check
                    // effectively this ensures one connection gets established before the next new client can start the
                    // connection process
                    let mut temp_connections_list = &mut connections_list_lock.lock().await;
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
                            Timer::new(),
                            connections_list_lock.clone(),
                            Connection::open(DB_LOCATION).unwrap()
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


