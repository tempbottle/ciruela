//! Client connection manager
//!
//! By cluster we mean just one or more ciruela servers which see each other
//! and have same directory namespace (no specific software known as
//! ciruela-cluster exists).
//!
//! We might expose individual server connections later, but now we only
//! have higher level API.

mod config;

pub use cluster::config::Config;

use std::sync::Arc;

use abstract_ns::{Name, Resolve, HostResolve};

use index::GetIndex;
use blocks::GetBlock;

/// Connection to a server or cluster of servers
///
/// Ciruela automatically manages a number of connections according to
/// configs and the operations over connection (i.e. images currently
/// uploading).
#[derive(Debug)]
pub struct Connection {
    config: Arc<Config>,
}

impl Connection {
    /// Create a connection pool object
    ///
    /// **Warning**: constructor should run on loop provieded by
    /// ``tk_easyloop``. In future, tokio will provide implicit loop reference
    /// on it's own.
    ///
    /// The actual underlying connections are established when specific
    /// operation is requested. Also you don't need to specify all node name
    /// in the cluster, they will be discovered.
    ///
    /// There are two common patterns:
    ///
    /// 1. [preferred] Use a DNS name that resolves to a full list of IP
    ///    addresses. Common DNS servers randomize them and only spill few
    ///    of the adressses because of DNS package size limit, but that's
    ///    fine, as we only need 3 or so of them.
    /// 2. Specify 3-5 server names and leave the discover to ciruela itself.
    ///
    /// While you can specify a name that refers to only one address, it's not
    /// a very good idea (unless you really have one server) because the server
    /// you're connecting to may fail.
    pub fn new<R, I, B>(initial_address: Vec<Name>, resolver: R,
        index_source: I, block_source: B)
        -> Connection
        where I: GetIndex + 'static,
              B: GetBlock + 'static,
              R: Resolve + HostResolve + 'static,
    {
        unimplemented!();
    }
}
