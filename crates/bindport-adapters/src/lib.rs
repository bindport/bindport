// SPDX-License-Identifier: MIT

use std::{
    collections::BTreeMap,
    fmt, fs,
    io::{self, Write},
    path::{Component, Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;

use bindport_core::{EffectiveOutputConfig, normalize_branch_label};
use minijinja::{AutoEscape, Environment, UndefinedBehavior};
use serde::Serialize;
use sha2::{Digest, Sha256};

mod hash;
mod kind;
mod ownership;
mod render;
mod resolver;
mod templates;

pub use hash::rendered_content_hash;
pub(crate) use hash::{content_hash, content_hash_matches, short_hash, stable_hash};
pub use kind::*;
pub use ownership::*;
pub use render::*;
pub use resolver::*;
pub use templates::*;

#[cfg(test)]
#[path = "unit_tests/mod.rs"]
mod tests;
