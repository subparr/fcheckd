#![allow(dead_code)]
#![allow(unused_imports)]
#![allow(unused_variables)]

use nix::sys::inotify::AddWatchFlags;
use std::path::PathBuf;
mod config;
use clap::Parser;
mod args;






fn main() {
    let cli_args = args::CliArgs::parse();
    let cfg = config::Cfg::init(Some(PathBuf::from("placeholder")));

}

