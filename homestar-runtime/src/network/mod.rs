//! libp2p, multi-use [HTTP] and [WebSocket] server, [ipfs], and [iroh] networking
//! interfaces.
//!
//! [HTTP]: jsonrpsee::server
//! [WebSocket]: jsonrpsee::server
//! [ipfs]: ipfs_api
//! [iroh]: iroh

pub(crate) mod error;
#[cfg(feature = "ipfs")]
#[cfg_attr(docsrs, doc(cfg(feature = "ipfs")))]
pub(crate) mod ipfs;
#[cfg(feature = "iroh")]
#[cfg_attr(docsrs, doc(cfg(feature = "iroh")))]
pub(crate) mod iroh;
pub(crate) mod pubsub;
pub mod rpc;
pub(crate) mod swarm;
pub(crate) mod webserver;

#[allow(unused_imports)]
pub(crate) use error::Error;
#[cfg(feature = "ipfs")]
#[cfg_attr(docsrs, doc(cfg(feature = "ipfs")))]
pub(crate) use ipfs::IpfsCli;
#[cfg(feature = "iroh")]
#[cfg_attr(docsrs, doc(cfg(feature = "iroh")))]
#[allow(unused_imports)]
pub(crate) use iroh::IrohEndpoint;
