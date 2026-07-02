// SPDX-License-Identifier: MIT

use std::{fmt, io};

use bindport_core::PortRange;

#[derive(Debug)]
pub enum RunnerError {
    NoCommand,
    NoAvailablePort { range: PortRange },
    SignalForwarding { source: io::Error },
    Spawn { command: String, source: io::Error },
    Wait { command: String, source: io::Error },
}

impl fmt::Display for RunnerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoCommand => write!(f, "no command provided after `--`"),
            Self::NoAvailablePort { range } => {
                write!(
                    f,
                    "no available port found in range {}-{}",
                    range.start, range.end
                )
            }
            Self::SignalForwarding { source } => {
                write!(f, "failed to install signal forwarding: {source}")
            }
            Self::Spawn { command, source } => {
                write!(f, "failed to spawn `{command}`: {source}")
            }
            Self::Wait { command, source } => {
                write!(f, "failed waiting for `{command}`: {source}")
            }
        }
    }
}

impl std::error::Error for RunnerError {}
