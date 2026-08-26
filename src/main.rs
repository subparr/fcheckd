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

//fn reload + delete watches from wd_to_script on watch death
//listen to IN_DELETE_SELF just for internal logic for wd_to_script?
//fn run hook 
//event loop
//recursive on inotify init 
//check flags on inotify init 
//logs to stdout or file?
//main before event loop successful init log + watched
//action log on verbose?

const INOTIFY_EPOLL_TOKEN: u64 = 0;
const SIGHUP_EPOLL_TOKEN: u64 = 1;

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

    let inotify_state = init_inotify(&cfg).unwrap_or_else(|err| {
            eprintln!("Inotify init/add_watch error: {err}"); 
            process::exit(1); 
    });

    drop(cfg);

    let sighup_fd = init_sighup().unwrap_or_else(|err| {
            eprintln!("Sighup init error: {err}"); 
            process::exit(1); 
    });
     
    let epoll_fd = init_epoll(&inotify_state, &sighup_fd).unwrap_or_else(|err| {
            eprintln!("Epoll init error: {err}"); 
            process::exit(1); 
    });
    
    event_loop(inotify_state, sighup_fd, epoll_fd);
    
}

fn event_loop(mut inotify_state: InotifyState, sighup_fd: SignalFd, epoll_fd: Epoll) { 
    let mut events = [EpollEvent::empty(); 2]; //2 for sighup in buf
                                               
    loop{
        let ctr = epoll_fd.wait(&mut events, EpollTimeout::NONE)?; //blocks here until events
        for event in &events[..ctr]{
            match event.data(){
                INOTIFY_EPOLL_TOKEN => run_script(&inotify_state)?, //Rc<>???
                SIGHUP_EPOLL_TOKEN => reload_cfg(&mut inotify_state)?, 
                _ => unreachable!("Unknown epoll token"),
            }
        }

    }
}

fn reload_cfg(inotify_state: &mut InotifyState) -> Result<(), Errno>{
    //init new cfg, check it
    //change wdtoscript on existing inotify fd in case cfg correct
    //check new cfg -> return bs -> nogo
    //create new cfg instance, reinit wdtoscript on existing notify fd
    //exit fn, don't propagate anything to main since flow is not returned there
    //THEN I DON'T ACTUALLY NEED THE OLD CFG
    //BUT THE DIFF CONFIGS WILL BE NEEDED WHEN MORE THAN INOTIFY IS UPDATED
    //AT THIS POINT JUST REMOVE WATCHES, ADD NEW ONES
    //PROB LESS EXPENSIVE THAN DIFFING CFGS ANYWAYS
    Ok(())
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

fn init_epoll(inotify_state: &InotifyState, sighup_fd: &SignalFd) -> Result<Epoll, Errno>{
    let epoll_fd = Epoll::new(EpollCreateFlags::EPOLL_CLOEXEC)?;

    epoll_fd.add(&inotify_state.inotify_fd, EpollEvent::new(EpollFlags::EPOLLIN, INOTIFY_EPOLL_TOKEN))?;
    epoll_fd.add(sighup_fd, EpollEvent::new(EpollFlags::EPOLLIN, SIGHUP_EPOLL_TOKEN))?;

    Ok(epoll_fd)
}

fn init_inotify(cfg: &Cfg) -> Result<InotifyState, Errno>{ //rewrite
    let mut inotify_state = InotifyState{
        //leave nonblock even for LT epoll just to be safe
        inotify_fd: Inotify::init(InitFlags::IN_CLOEXEC | InitFlags::IN_NONBLOCK)?,
        wd_to_script: HashMap::new(),
    };

    for entry in &cfg.entry{
        let cycle_wd = inotify_state.inotify_fd.
            add_watch(&entry.path, entry.events)?;
        inotify_state.wd_to_script.
           insert(cycle_wd, entry.path.clone()); 
    }
    Ok(inotify_state)

}

fn update_inotify(inotify_state: &mut InotifyState) -> Result<(),Errno>{
    Ok(())
}


fn init_sighup() -> Result<SignalFd, Errno> { //add struct for storing signalfds if several introduced
    let mut mask = SigSet::empty();
    mask.add(Signal::SIGHUP);
    sigprocmask(SigmaskHow::SIG_BLOCK, Some(&mask), None)?; //clear before script exec
    
    let sighup_fd = SignalFd::with_flags(&mask, SfdFlags::SFD_CLOEXEC | SfdFlags::SFD_NONBLOCK)?;
    Ok(sighup_fd)
}

