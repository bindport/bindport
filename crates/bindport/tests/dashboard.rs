// SPDX-License-Identifier: MIT

mod support;

#[cfg(unix)]
#[path = "dashboard/service_support.rs"]
mod service_support;

#[cfg(unix)]
#[path = "dashboard/hooks.rs"]
mod hooks;
#[cfg(unix)]
#[path = "dashboard/shutdown.rs"]
mod shutdown;

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
