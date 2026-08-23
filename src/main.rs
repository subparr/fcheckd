#![allow(dead_code)]
#![allow(unused_imports)]
#![allow(unused_variables)]

use std::process;
use nix::sys::inotify::{Inotify, AddWatchFlags, InitFlags};
use nix::sys::epoll;
use nix::errno::Errno;
use std::path::PathBuf;
mod config;
use config::{Cfg, CfgEntry};

use clap::Parser;
mod args;





//store config in mem for cfg reload on wrong config + fn for reload 
fn main() {
    let cli_args = args::CliArgs::parse();
    let cfg = Cfg::init(cli_args.config).unwrap_or_else(|err| {
            eprintln!("Error parsing config: {err}");
            process::exit(1); //drops the process on cfg reload if logic is reused for it, rewrite
    });



}

fn event_loop(){}

fn init_epoll(){}

fn init_inotify(cfg: &Cfg) -> Result<Inotify, Errno>{
    let inotify_fd = Inotify::init(InitFlags::empty())?;
    for entry in &cfg.entry{
        inotify_fd.add_watch("{&entry.path}", entry.events)?;
    }
    Ok(inotify_fd) //wd actually? tf is the diff
}
fn init_signal(){}

fn init_cfg_parse_flags(){}
fn reload(){}
