mod outgoing;
pub mod websocket;

pub use remote::websocket::Connection;

use std::net::SocketAddr;
use std::collections::{HashSet, HashMap};
use std::sync::{Arc};
use std::time::{Duration};

use ciruela::proto::{ReceivedImage};
use ciruela::proto::{Registry};
use ciruela::{ImageId, VPath};
use failure_tracker::HostFailures;
use named_mutex::{Mutex, MutexGuard};
use remote::outgoing::connect;
use tk_http::websocket::{Config as WsConfig};
use tracking::Tracking;


pub struct Connections {
    // TODO(tailhook) optimize incoming and outgoing connections
    // but keep in mind that client connections should never be used as
    // outgoing (i.e. you can use `ciruela upload` on the same host as
    // `ciruela-server`)
    incoming: HashSet<Connection>,
    outgoing: HashMap<SocketAddr, Connection>,
    failures: HostFailures,
    declared_images: HashMap<ImageId, HashSet<Connection>>,
}

pub struct Token(Remote, Connection);

#[derive(Clone)]
pub struct Remote(Arc<RemoteState>);

struct RemoteState {
    websock_config: Arc<WsConfig>,
    conn: Mutex<Connections>,
}


impl Remote {
    pub fn new()
        -> Remote
    {
        Remote(Arc::new(RemoteState {
            websock_config: WsConfig::new()
                .ping_interval(Duration::new(1200, 0)) // no pings
                .inactivity_timeout(Duration::new(5, 0))
                .done(),
            conn: Mutex::new(Connections {
                incoming: HashSet::new(),
                outgoing: HashMap::new(),
                failures: HostFailures::new_default(),
                declared_images: HashMap::new(),
            }, "remote_connections"),
        }))
    }
    pub fn websock_config(&self) -> &Arc<WsConfig> {
        &self.0.websock_config
    }
    fn inner(&self) -> MutexGuard<Connections> {
        self.0.conn.lock()
    }
    pub fn register_connection(&self, cli: &Connection) -> Token {
        self.inner().incoming.insert(cli.clone());
        return Token(self.clone(), cli.clone());
    }
    pub fn get_incoming_connection_for_image(&self, id: &ImageId)
        -> Option<Connection>
    {
        self.inner().declared_images.get(id)
            .and_then(|x| x.iter().cloned().next())
    }
    /// Splits the list into connected/not-connected list *and* removes
    /// recently failed addresses
    pub fn split_connected<I: Iterator<Item=SocketAddr>>(&self, inp: I)
        -> (Vec<Connection>, Vec<SocketAddr>)
    {
        let mut conn = Vec::new();
        let mut not_conn = Vec::new();
        let state = self.inner();
        for sa in inp {
            if let Some(con) = state.outgoing.get(&sa) {
                conn.push(con.clone());
            } else if state.failures.can_try(sa) {
                not_conn.push(sa);
            }
        }
        return (conn, not_conn);
    }
    pub fn ensure_connected(&self, tracking: &Tracking, addr: SocketAddr)
        -> Connection
    {
        if let Some(conn) = self.inner().outgoing.get(&addr) {
            return conn.clone();
        }
        let reg = Registry::new();
        let (cli, rx) = Connection::outgoing(addr, &reg);
        self.inner().outgoing.insert(addr, cli.clone());
        let tok = Token(self.clone(), cli.clone());
        connect(self, tracking, &reg, cli.clone(), tok, addr, rx);
        cli.clone()
    }
    pub fn notify_received_image(&self, id: &ImageId, path: &VPath) {
        for conn in self.inner().incoming.iter() {
            if conn.has_image(id) {
                conn.notification(ReceivedImage {
                    id: id.clone(),
                    // TODO(tailhook)
                    hostname: String::from("localhost"),
                    forwarded: false,
                    path: path.clone(),
                })
            }
        }
    }
}

impl Drop for Token {
    fn drop(&mut self) {
        let mut remote = self.0.inner();
        if self.1.hanging_requests() > 0 || !self.1.is_connected() {
            remote.failures.add_failure(self.1.addr());
        }
        if remote.incoming.remove(&self.1) {
            for img in self.1.images().iter() {
                let left = {
                    let mut item = remote.declared_images.get_mut(img);
                    item.as_mut().map(|x| x.remove(&self.1));
                    item.map(|x| x.len())
                };
                match left {
                    Some(0) => {
                        remote.declared_images.remove(img);
                    }
                    _ => {}
                }
            }
        }
        if remote.outgoing.get(&self.1.addr())
            .map(|x| *x == self.1).unwrap_or(false)
        {
            remote.outgoing.remove(&self.1.addr());
        }
    }
}
