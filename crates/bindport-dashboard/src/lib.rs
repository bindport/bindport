// SPDX-License-Identifier: MIT

use std::{
    borrow::Cow,
    fmt, fs,
    io::{self, BufRead, BufReader, Write},
    net::{Ipv4Addr, SocketAddrV4, TcpListener, TcpStream},
    path::{Path, PathBuf},
    sync::Arc,
    thread,
    time::Duration,
};

use bindport_core::{DEFAULT_PORT_RANGE, DEFAULT_SKIP_PORTS, PortRange};
use bindport_registry::{CleanState, CleanSummary, Registry};

mod assets;
mod auth;
mod constants;
mod error;
mod options;
mod request;
mod response;
mod routing;
mod server;

pub(crate) use assets::*;
pub(crate) use auth::*;
pub use constants::*;
pub use error::*;
pub use options::*;
pub(crate) use request::*;
pub(crate) use response::*;
pub(crate) use routing::*;
pub use server::*;

#[cfg(test)]
#[path = "unit_tests/mod.rs"]
mod tests;
