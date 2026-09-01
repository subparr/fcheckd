#![allow(dead_code)]
#![allow(unused_imports)]
#![allow(unused_variables)]

#![allow(clippy::zombie_processes)]

use std::process;
use nix::sys::inotify::{Inotify, AddWatchFlags, InitFlags, InotifyEvent, WatchDescriptor};
use std::collections::HashMap;
use nix::sys::epoll::{Epoll, EpollEvent, EpollCreateFlags, EpollFlags, EpollTimeout};
use nix::errno::Errno;
use std::path::PathBuf;
use nix::sys::signal::{SigSet, Signal, sigprocmask, SigmaskHow};
use nix::sys::signalfd::{SignalFd, SfdFlags};
use std::process::{Command, Child, Stdio};
use std::os::unix::process::CommandExt;
use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};
use nix::unistd::Pid;

mod config;
use config::{Cfg, CfgEntry};

use clap::Parser;
mod args;
use crate::args::CliArgs;    

//handle IN_Q_OVERFLOW on wd -1
//recursive on inotify init for dirs 
//logs to file? /var/log/fcheckd/err_log and all_log
//comprehensive logging
//action log on verbose?

const INOTIFY_EPOLL_TOKEN: u64 = 0;
const SIGNAL_EPOLL_TOKEN: u64 = 1;
const EPOLL_BUF_LEN: usize = 2;

//wd_to_script may contain invalid entries after move/delete/umount
//but events will not be registered for those files after the invalidation
//see  https://man7.org/linux/man-pages/man7/inotify.7.html
//any pending events before invalidation are available 
//and WILL be processed
struct InotifyState {
    inotify_fd: Inotify,
    wd_to_script: HashMap<WatchDescriptor, PathBuf>,
}

impl InotifyState {
    fn del_watches(&mut self) {
        for (wd, _) in self.wd_to_script.drain() {
                self.inotify_fd.rm_watch(wd).unwrap_or_else(|err| {
                    eprintln!("Error removing watch: {err}") //EINVAL, can happen on IN_IGNORED
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

    let signal_fd = init_signals().unwrap_or_else(|err| {
            eprintln!("Sighup init error: {err}"); 
            process::exit(1); 
    });
     
    let epoll_fd = init_epoll(&inotify_state, &signal_fd).unwrap_or_else(|err| {
            eprintln!("Epoll init error: {err}"); 
            process::exit(1); 
    });
    
    event_loop(inotify_state, signal_fd, epoll_fd, cli_args, cfg);
    
}

fn event_loop(mut inotify_state: InotifyState, mut signal_fd: SignalFd, epoll_fd: Epoll, cli_args: CliArgs, mut cfg: Cfg) { 
    let mut events = [EpollEvent::empty(); EPOLL_BUF_LEN];
                                               
    loop{
        let ctr = match epoll_fd.wait(&mut events, EpollTimeout::NONE){
            Ok(ctr) => ctr,
            Err(Errno::EINTR) => continue, //shouldn't EVER happen, nothing to be interrupted by
            Err(err) => {
                eprintln!("Epoll failed unexpectedly: {err}");
                process::exit(1);
            },
        }; //blocks here until events 
           
        for event in &events[..ctr]{
            match event.data(){
                INOTIFY_EPOLL_TOKEN => handle_inotify(&inotify_state), 
                SIGNAL_EPOLL_TOKEN =>  handle_signalfd(&mut signal_fd, &mut inotify_state, &cli_args, &mut cfg),
                _ => unreachable!("Unknown epoll token"),
            }
        }
    }
}


fn handle_inotify(inotify_state: &InotifyState) {
    loop{
        match inotify_state.inotify_fd.read_events(){
            Ok(events) => {
                for event in events {
                    //IN_ONESHOT caused script to fire twice because existing 
                    //wd_to_script entry, ignore signal omitted by rm_watch
                    if event.mask.contains(AddWatchFlags::IN_IGNORED) {
                        continue;
                    }
                    
                    let script = match inotify_state.wd_to_script.get(&event.wd){
                        Some(script) => script,
                        None => {
                            eprintln!("No script path found for inotify wd. Skipping."); //Internal?
                            continue;
                        },
                    };

                    let mut command = Command::new(script);
                    command
                        .stdin(Stdio::null())
//                        .stdout(Stdio::null())
                        .stderr(Stdio::null());
                    
                    let unblock_mask = Signal::SIGHUP | Signal::SIGCHLD;

                    unsafe {
                        command.pre_exec(move || {
                            sigprocmask(SigmaskHow::SIG_UNBLOCK, Some(&unblock_mask), None)
                                .map_err(std::io::Error::from)
                        });
                    }

                    //don't listen to fucking clippy, it doesn't know shit,
                    //process is waited on down the line when it's finished
                    //to not block the single thread this daemon barely hangs on
                    match command.spawn(){
                        Ok(_) => {},
                        Err(err) => {
                            eprintln!("Error spawning a child script {script:?}: {err}");
                        },
                    };

                }
            },
            Err(Errno::EAGAIN) => break,
            Err(Errno::EINTR) => continue,
            Err(err) => {
                eprintln!("Error reading inotify event(s): {err}");
                process::exit(1);
            },
        }
    }
}

//continue on error and log it to stderr
fn handle_signalfd(signal_fd: &mut SignalFd, inotify_state: &mut InotifyState, cli_args: &CliArgs, cfg: &mut Cfg) {
    loop{
        match signal_fd.read_signal() {
             Ok(signal) => match signal {
                Some(signal) => {
                    match Signal::try_from(signal.ssi_signo as i32) {
                        Ok(Signal::SIGHUP) => {
                            reload_cfg(inotify_state, cli_args, cfg);     
                        }
                        Ok(Signal::SIGCHLD) => {
                            handle_children();
                        }
                        Ok(other) => {
                            unreachable!("Unexpected signal: {other}");
                        }
                        Err(err) => {
                            unreachable!("Unknown signal number: {err}");
                        }
                    }
                },
                None => break, 
             },
             Err(err) => eprintln!("Unable to read signal: {err}")
        };

    }
}

//ambatu blow watafak loops
fn handle_children(){
    loop{
        match waitpid(Pid::from_raw(-1), Some(WaitPidFlag::WNOHANG)){
            Ok(WaitStatus::StillAlive) => break, 
            Err(Errno::ECHILD) => break,
            _ => {}, 
        };

    }
}

fn reload_cfg(inotify_state: &mut InotifyState, cli_args: &CliArgs, cfg: &mut Cfg){

    match Cfg::init(cli_args.config.as_deref()) {
        Ok(cfg_new) => *cfg = cfg_new,
        Err(err) => eprintln!("Error parsing new config, reloading with the previous one: {err}"),
    }

    update_inotify(inotify_state, cfg);
}

fn update_inotify(inotify_state: &mut InotifyState, cfg: &Cfg) {
    inotify_state.del_watches();
    inotify_fill_from_cfg(inotify_state, cfg);
}

//continue on error and log it to stderr
fn inotify_fill_from_cfg(inotify_state: &mut InotifyState, cfg: &Cfg) {
    for entry in &cfg.entry{
        match inotify_state.inotify_fd.add_watch(&entry.path, entry.events){
            Ok(wd) => {
                inotify_state.wd_to_script.insert(wd, entry.script.clone());
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


//don't check for directory specific flags as they get ignored and vice versa for
//file specifi flags on dirs
fn init_inotify(cfg: &Cfg) -> Result<InotifyState, Errno> { 
    let mut inotify_state = InotifyState{
        inotify_fd: Inotify::init(InitFlags::IN_CLOEXEC | InitFlags::IN_NONBLOCK)?,
        wd_to_script: HashMap::new(),
    };

    inotify_fill_from_cfg(&mut inotify_state, cfg);
    Ok(inotify_state)
}


fn init_epoll(inotify_state: &InotifyState, signal_fd: &SignalFd) -> Result<Epoll, Errno>{

    let epoll_fd = Epoll::new(EpollCreateFlags::EPOLL_CLOEXEC)?;

    epoll_fd.add(&inotify_state.inotify_fd, EpollEvent::new(EpollFlags::EPOLLIN, INOTIFY_EPOLL_TOKEN))?;
    epoll_fd.add(signal_fd, EpollEvent::new(EpollFlags::EPOLLIN, SIGNAL_EPOLL_TOKEN))?;

    Ok(epoll_fd)
}


fn init_signals() -> Result<SignalFd, Errno> {
    let mask = Signal::SIGHUP | Signal::SIGCHLD;
    sigprocmask(SigmaskHow::SIG_BLOCK, Some(&mask), None)?;
    
    let signal_fd = SignalFd::with_flags(&mask, SfdFlags::SFD_CLOEXEC | SfdFlags::SFD_NONBLOCK)?;
    Ok(signal_fd)
}

