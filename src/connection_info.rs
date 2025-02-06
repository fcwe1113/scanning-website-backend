use std::net::SocketAddr;

#[derive(Debug, Clone)]
pub(crate) struct ConnectionInfo {
    pub(crate) addr: SocketAddr,
    pub(crate) token: String,
    // add more crap here later
}

impl ConnectionInfo {
    pub(crate) fn new(addr: SocketAddr, token: String) -> ConnectionInfo {
        ConnectionInfo { addr, token }
    }
}