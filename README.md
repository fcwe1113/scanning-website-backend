# Scanning website backend

This is my the backend of my thesis project, which is a supermarket barcode/QR code scanner designed to better shopping experiences. This is intended to be paired with the frontend, which is also available at https://github.com/fcwe1113/scanning-website

# How To Run

## List of dependencies:
anyhow

futures-util

tokio

tokio-tungstenite

tokio-rustls

tungstenite

rustls

tracing-subscriber

log

rand

rand_chacha

chrono

rusqlite

argon2

serde

serde-json

serde-email

creditcard

regex

## Step by step setup

1. ensure you have rust installed, if not download it from here: https://www.rust-lang.org/tools/install
2. choose the destination folder you want the backend to be in then clone the project, or get Github Desktop (https://desktop.github.com/download/) to do it for you
3. in a command prompt window (a brand new one if you are using one already) navigate to the cloned repository and run ``` cargo run ```
4. rust will then install all the required packages automatically, then compile it, then run it

# Local only mode

As this site is built for mobile devices running it in localhost only mode is missing the point, but if you still choose to do so, please switch to the no-tls branch (https://github.com/fcwe1113/scanning-website-backend/tree/no-tls) and run that version instead. And assuming the frontend is configured accordingly, you can run it in local only mode
