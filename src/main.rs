#![allow(dead_code)]
#![allow(unused_imports)]
#![allow(unused_variables)]

use std::process;
use nix::sys::inotify::{Inotify, AddWatchFlags, InitFlags, InotifyEvent, WatchDescriptor};
use std::collections::HashMap;
use nix::sys::epoll::{Epoll, EpollEvent, EpollCreateFlags, EpollFlags, EpollTimeout};
use nix::errno::Errno;
use std::path::PathBuf;
use nix::sys::signal::{SigSet, Signal, sigprocmask, SigmaskHow};
use nix::sys::signalfd::{SignalFd, SfdFlags};

mod config;
use config::{Cfg, CfgEntry};

use clap::Parser;
mod args;
use crate::args::CliArgs;    

//fn reload + delete watches from wd_to_script on watch death
//listen to IN_DELETE_SELF just for internal logic for wd_to_script?
//recursive on inotify init 
//check flags on inotify init 
//logs to stdout or file?
//action log on verbose?

const INOTIFY_EPOLL_TOKEN: u64 = 0;
const SIGHUP_EPOLL_TOKEN: u64 = 1;
const EPOLL_BUF_LEN: usize = 2;

struct InotifyState {
    inotify_fd: Inotify,
    wd_to_script: HashMap<WatchDescriptor, PathBuf>,
}

impl InotifyState {
    fn del_watches(&mut self) {
        for (wd, _) in self.wd_to_script.drain() {
                self.inotify_fd.rm_watch(wd).unwrap_or_else(|err| {
                    eprintln!("Error removing watch: {err}") //EINVAL, can happen on move/delete
                });
            }
    }

}

fn main() {
    let cli_args = args::CliArgs::parse();
    let cfg = Cfg::init(cli_args.config.as_deref()).unwrap_or_else(|err| {
            eprintln!("Error parsing config: {err}");
            process::exit(1);
    });

    if cli_args.check_config {
        println!("OK!");
        process::exit(0);
    }

    let inotify_state = init_inotify(&cfg).unwrap_or_else(|err| {
            eprintln!("Inotify init/add_watch error: {err}"); 
            process::exit(1); 
    });

    let sighup_fd = init_sighup().unwrap_or_else(|err| {
            eprintln!("Sighup init error: {err}"); 
            process::exit(1); 
    });
     
    let epoll_fd = init_epoll(&inotify_state, &sighup_fd).unwrap_or_else(|err| {
            eprintln!("Epoll init error: {err}"); 
            process::exit(1); 
    });
    
    event_loop(inotify_state, sighup_fd, epoll_fd, cli_args, cfg);
    
}

fn event_loop(mut inotify_state: InotifyState, sighup_fd: SignalFd, epoll_fd: Epoll, cli_args: CliArgs, mut cfg: Cfg) { 
    let mut events = [EpollEvent::empty(); EPOLL_BUF_LEN];
                                               
    loop{
        let ctr = epoll_fd.wait(&mut events, EpollTimeout::NONE).unwrap(); //blocks here until events 
        for event in &events[..ctr]{
            match event.data(){
                INOTIFY_EPOLL_TOKEN => run_script(&inotify_state).unwrap(), //RESOLVE UNWRAPS + gets
                                                                            //IN_IGNORED?
                SIGHUP_EPOLL_TOKEN => cfg = reload_cfg(&mut inotify_state, &cli_args, cfg),
                _ => unreachable!("Unknown epoll token"),
            }
        }
    }
}

fn run_script(inotify_state: &InotifyState) -> Result<(), Errno>{
    loop{
        match inotify_state.inotify_fd.read_events(){
            Ok(events) => {
                for event in events {
                    let script = inotify_state.wd_to_script.get(&event.wd);
                    // script goes brrrrrrr
                }
            },
            Err(Errno::EAGAIN) => break Ok(()),
            Err(e) => return Err(e),
        }
    }
}

fn reload_cfg(inotify_state: &mut InotifyState, cli_args: &CliArgs, cfg_old: Cfg) -> Cfg{
    let cfg = match Cfg::init(cli_args.config.as_deref()) {
        Ok(cfg_new) => cfg_new,
        Err(err) => {
            eprintln!("Error parsing new config, reloading with the previous one: {err}");
            cfg_old
        },
    };

    update_inotify(inotify_state, &cfg);
    cfg  
}

fn init_inotify(cfg: &Cfg) -> Result<InotifyState, Errno> { //rewrite as impl for InotifyState??
    let mut inotify_state = InotifyState{
        inotify_fd: Inotify::init(InitFlags::IN_CLOEXEC | InitFlags::IN_NONBLOCK)?,
        wd_to_script: HashMap::new(),
    };

    inotify_fill_from_cfg(&mut inotify_state, cfg);
    Ok(inotify_state)
}

fn update_inotify(inotify_state: &mut InotifyState, cfg: &Cfg) {
    inotify_state.del_watches();
    inotify_fill_from_cfg(inotify_state, cfg);
}

fn inotify_fill_from_cfg(inotify_state: &mut InotifyState, cfg: &Cfg) {
    for entry in &cfg.entry{
        match inotify_state.inotify_fd.add_watch(&entry.path, entry.events){
            Ok(wd) => {
                inotify_state.wd_to_script.insert(wd, entry.path.clone());
            },
            Err(err) => {
                eprintln!("Error adding watch for {:?}: {err}.", entry.path)
            },
        }
    }
    let active = inotify_state.wd_to_script.len();
    let total = cfg.entry.len();
    if active < total {
        eprintln!("Warning: {active}/{total} watches active");
    }
}


fn init_epoll(inotify_state: &InotifyState, sighup_fd: &SignalFd) -> Result<Epoll, Errno>{

    let epoll_fd = Epoll::new(EpollCreateFlags::EPOLL_CLOEXEC)?;

    epoll_fd.add(&inotify_state.inotify_fd, EpollEvent::new(EpollFlags::EPOLLIN, INOTIFY_EPOLL_TOKEN))?;
    epoll_fd.add(sighup_fd, EpollEvent::new(EpollFlags::EPOLLIN, SIGHUP_EPOLL_TOKEN))?;

    Ok(epoll_fd)
}


fn init_sighup() -> Result<SignalFd, Errno> { //add struct for storing signalfds if several introduced
    let mut mask = SigSet::empty();
    mask.add(Signal::SIGHUP);
    sigprocmask(SigmaskHow::SIG_BLOCK, Some(&mask), None)?; //clear before script exec
    
    let sighup_fd = SignalFd::with_flags(&mask, SfdFlags::SFD_CLOEXEC | SfdFlags::SFD_NONBLOCK)?;
    Ok(sighup_fd)
}

