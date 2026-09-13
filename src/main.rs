// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 subparr <subparr@tuta.io>

use std::process;

use fcheckd::config::Cfg;
use fcheckd::logger::Logger;
use fcheckd::{event_loop, init_epoll, init_inotify, init_signals};

use clap::Parser;
mod args;
use args::CliArgs;

//Initialise everything, pass it to event loop
fn main() {
    let cli_args = CliArgs::parse();

    Logger::init(cli_args.verbose, &cli_args.log_file);

    let cfg = Cfg::init(&cli_args.config).unwrap_or_else(|err| {
        Logger::fatal(format!("Error parsing config: {err}"));
    });

    if cli_args.check_config {
        println!("OK!");
        process::exit(0);
    }

    drop(cli_args);

    let inotify_state = init_inotify(&cfg).unwrap_or_else(|err| {
        Logger::fatal(format!("Inotify init/add_watch error: {err}"));
    });

    let signal_fd = init_signals().unwrap_or_else(|err| {
        Logger::fatal(format!("Sighup init error: {err}"));
    });

    let epoll_fd = init_epoll(&inotify_state, &signal_fd).unwrap_or_else(|err| {
        Logger::fatal(format!("Epoll init error: {err}"));
    });

    event_loop(inotify_state, signal_fd, epoll_fd, cfg);
}
