use std::net::SocketAddr;
use crate::screen_state::ScreenState;

#[derive(Debug, Clone)]
pub(crate) struct ConnectionInfo {
    pub(crate) client_addr: SocketAddr,
    pub(crate) token: String,
    pub(crate) token_exchanged: bool,
    pub(crate) screen: ScreenState,
    pub(crate) username: String,
    // add more crap here later
}

impl ConnectionInfo {
    pub(crate) fn new(addr: SocketAddr, token: String) -> ConnectionInfo {
        ConnectionInfo { client_addr: addr, token, token_exchanged: false, screen: ScreenState::TokenHandshake, username: "".to_string() }
    }
}