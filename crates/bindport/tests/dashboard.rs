// SPDX-License-Identifier: MIT

mod support;

#[path = "dashboard/clean.rs"]
mod clean;
#[cfg(unix)]
#[path = "dashboard/lifecycle.rs"]
mod lifecycle;
#[path = "dashboard/options.rs"]
mod options;
#[path = "dashboard/ports.rs"]
mod ports;
#[path = "dashboard/registration.rs"]
mod registration;
#[path = "dashboard/routing.rs"]
mod routing;
#[path = "dashboard/service.rs"]
mod service;
#[path = "dashboard/status.rs"]
mod status;
