// SPDX-License-Identifier: MIT

use std::{
    env, fmt, fs,
    io::{self, Read, Write},
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream},
    path::{Path, PathBuf},
    time::Duration,
};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use bindport_core::{SERVICE_NAME, ServiceIdentity};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::Serialize;

mod cleanup;
mod clock;
mod connection;
mod constants;
mod error;
mod health;
mod lease;
mod outputs;
mod process;
mod schema;
mod status;

pub use cleanup::*;
pub(crate) use clock::*;
pub use connection::*;
pub use constants::*;
pub use error::*;
pub(crate) use health::*;
pub use lease::*;
pub use outputs::*;
pub(crate) use process::*;
pub use status::*;

#[cfg(test)]
#[path = "unit_tests/mod.rs"]
mod tests;
