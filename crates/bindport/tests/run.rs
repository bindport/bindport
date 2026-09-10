// SPDX-License-Identifier: MIT

mod support;

#[path = "run/allocation.rs"]
mod allocation;
#[path = "run/basic.rs"]
mod basic;
#[path = "run/configured.rs"]
mod configured;
#[path = "run/cross_service.rs"]
mod cross_service;
#[path = "run/cross_service_reservation.rs"]
mod cross_service_reservation;
#[path = "run/environment.rs"]
mod environment;
#[path = "run/local_bin.rs"]
mod local_bin;
#[path = "run/package.rs"]
mod package;
#[path = "run/reserved.rs"]
mod reserved;
#[path = "run/signals.rs"]
mod signals;
#[path = "run/templates.rs"]
mod templates;
