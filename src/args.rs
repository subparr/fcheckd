// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 subparr <subparr@tuta.io>

use clap::Parser;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    version,
    about = "Watch files and react to their changes",
    after_help = "Report bugs to subparr@tuta.io\n"
)]
pub struct CliArgs {
    ///Path to config
    #[arg(short, long)]
    pub config: Option<PathBuf>,

    ///Check config, report to stdout and exit
    #[arg(short = 'e', long)]
    pub check_config: bool,

    ///Make output more verbose
    #[arg(short = 'v', long)]
    pub verbose: bool,

    ///Write log to a file instead of stderr/stdout
    #[arg(short = 'l', long)]
    pub log_file: Option<PathBuf>,
}
