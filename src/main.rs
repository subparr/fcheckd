#![allow(dead_code)]
#![allow(unused_imports)]
#![allow(unused_variables)]

use std::process;
use nix::sys::inotify::{Inotify, AddWatchFlags, InitFlags, InotifyEvent, WatchDescriptor};
use std::collections::HashMap;
use nix::sys::epoll::{Epoll, EpollEvent, EpollCreateFlags};
use nix::errno::Errno;
use std::path::PathBuf;
mod config;
use config::{Cfg, CfgEntry};

use clap::Parser;
mod args;

struct InotifyState {
    inotify_fd: Inotify,
    wd_to_script: HashMap<WatchDescriptor, PathBuf>,
}



//store config in mem for cfg reload on wrong config + fn for reload 
fn main() {
    let cli_args = args::CliArgs::parse();
    let cfg = Cfg::init(cli_args.config).unwrap_or_else(|err| {
            eprintln!("Error parsing config: {err}");
            process::exit(1); //drops the process on cfg reload if logic is reused for it, rewrite
    });

    if cli_args.check_config {
        println!("OK!");
        process::exit(0);
    }

    let inotify_wd = init_inotify(&cfg).unwrap_or_else(|err| {
            eprintln!("Inotify init/add_watch error: {err}"); 
            process::exit(1); 
    });

    let epoll_fd = init_epoll().unwrap_or_else(|err| {
            eprintln!("Epoll init error: {err}"); 
            process::exit(1); 
    });
}




fn event_loop(inotify_fd: Inotify, epoll_fd: Epoll){
    loop{
        
    }
}

fn init_epoll() -> Result<Epoll, Errno>{
    let epoll_fd = Epoll::new(EpollCreateFlags::EPOLL_CLOEXEC)?;
    
    Ok(epoll_fd)
}


fn init_inotify(cfg: &Cfg) -> Result<InotifyState, Errno>{
    let mut inotify_state = InotifyState{
        inotify_fd: Inotify::init(InitFlags::IN_CLOEXEC | InitFlags::IN_NONBLOCK)?, //Errno EAGAIN for epoll
        wd_to_script: HashMap::new(),
    };

    for entry in &cfg.entry{
        let cycle_wd = inotify_state.inotify_fd.
            add_watch(&entry.path, entry.events)?;
        inotify_state.wd_to_script.
            insert(cycle_wd, entry.path.clone()); //clone burger shitcode
    }
    Ok(inotify_state)
}

fn init_signal(){}


fn reload(){}
