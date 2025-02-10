// Copyright 2023 the Strata authors
// SPDX-License-Identifier: GPL-3.0-or-later

#![feature(generic_arg_infer)]

use std::io::stdout;

use chrono::Local;
use clap::Parser;
use log::info;
use smithay::reexports::{
	calloop::EventLoop,
	wayland_server::Display,
};
use tracing_subscriber::fmt::writer::MakeWriterExt;

use crate::{
	backends::Backend,
	state::{
		Compositor,
		Strata,
	},
};

pub mod api;
pub mod backends;
pub mod config;
pub mod decorations;
pub mod handlers;
pub mod layouts;
pub mod state;
pub mod tiling;
pub mod util;
pub mod workspaces;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Args {
	#[arg(short, long)]
	pub backend: String,
}

fn main() -> anyhow::Result<()> {
	let args = Args::parse();
	let xdg = xdg::BaseDirectories::with_prefix("strata")?;
	let log_dir = xdg.get_state_home();

	let file_appender = tracing_appender::rolling::never(
		&log_dir,
		format!("strata_{}.log", Local::now().format("%Y-%m-%d_%H:%M:%S")),
	);

	let latest_file_appender = tracing_appender::rolling::never(&log_dir, "latest.log");
	let log_appender = stdout.and(file_appender).and(latest_file_appender);

	if let Ok(env_filter) = tracing_subscriber::EnvFilter::try_from_default_env() {
		tracing_subscriber::fmt()
			.with_writer(log_appender)
			.with_env_filter(env_filter)
			.init();
	} else {
		tracing_subscriber::fmt().with_writer(log_appender).init();
	}

	info!("Initializing Strata WM");

	let mut event_loop = EventLoop::try_new()?;
	let mut comp = Compositor::new(&event_loop)?;
	comp.backend = Backend::from_str(&args.backend, &mut comp)?;

	let mut state = Strata::new(comp)?;
	event_loop.run(None, &mut state, move |strata| {
		match strata.comp.display_handle.flush_clients() {
			Ok(_) => {},
			Err(e) => println!("Display I/O Error: {:?}", e),
};
	})?;

	info!("Quitting Strata WM");

	Ok(())
}
