#![allow(clippy::zombie_processes)]
#![allow(unused)]

use std::process;
use nix::sys::inotify::{Inotify, AddWatchFlags, InitFlags, WatchDescriptor};
use std::collections::HashMap;
use std::iter;
use std::rc::Rc;
use nix::sys::epoll::{Epoll, EpollEvent, EpollCreateFlags, EpollFlags, EpollTimeout};
use nix::errno::Errno;
use std::path::{Path, PathBuf};
use nix::sys::signal::{Signal, sigprocmask, SigmaskHow};
use nix::sys::signalfd::{SignalFd, SfdFlags};
use std::process::{Command, Stdio};
use std::os::unix::process::CommandExt;
use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};
use nix::unistd::Pid;

mod config;
use config::Cfg;

use clap::Parser;
mod args;
use args::CliArgs;    

mod logger;
use logger::Logger;


//readme
//license
//exclude features
//copyrights
//cargo deny 
//cargo audit
//cargo AUR + cargo deb + install.sh
//systemd unit file
//openrc service file

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
    wd_to_script: HashMap<WatchDescriptor, Rc<PathBuf>>,
}

impl InotifyState {
    fn del_watches(&mut self) {
        for (wd, _) in self.wd_to_script.drain() {
                self.inotify_fd.rm_watch(wd).unwrap_or_else(|err| {
                    Logger::error(format!("Error removing watch: {err}")); //EINVAL, can happen on IN_IGNORED
                });
            }
    }

}

fn main() {
    let cli_args = CliArgs::parse();

    Logger::init(cli_args.verbose, cli_args.log_file);

    let cfg = Cfg::init(&cli_args.config).unwrap_or_else(|err| {
        Logger::fatal(format!("Error parsing config: {err}"));
    });

    if cli_args.check_config { 
        println!("OK!");
        process::exit(0);
    }
    
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

fn event_loop(mut inotify_state: InotifyState, mut signal_fd: SignalFd, epoll_fd: Epoll, mut cfg: Cfg) { 
    let mut events = [EpollEvent::empty(); EPOLL_BUF_LEN];
                                               
    loop{
        let ctr = match epoll_fd.wait(&mut events, EpollTimeout::NONE){
            Ok(ctr) => ctr,
            Err(Errno::EINTR) => continue, //shouldn't EVER happen, nothing to be interrupted by
            Err(err) => {
                Logger::fatal(format!("Epoll failed unexpectedly: {err}"));
            },
        }; //blocks here until events 
           
        for event in &events[..ctr]{
            match event.data(){
                INOTIFY_EPOLL_TOKEN => handle_inotify(&inotify_state), 
                SIGNAL_EPOLL_TOKEN =>  handle_signalfd(&mut signal_fd, &mut inotify_state, &mut cfg),
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
                    if event.mask.contains(AddWatchFlags::IN_Q_OVERFLOW) {
                       eprintln!("Inotify event queue is overflowed. Some events may be dropped"); 
                    }
                    
                    let script = match inotify_state.wd_to_script.get(&event.wd){
                        Some(script) => script,
                        None => {
                            Logger::error("No script path found for inotify wd. Skipping"); //Internal?
                            continue;
                        },
                    };

                    let mut command = Command::new(script.as_ref());
                    command
                        .stdin(Stdio::null())
                        .stdout(Stdio::null())
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
                            Logger::error(format!("Error spawning a child script {script:?}: {err}"));
                        },
                    };

                }
            },
            Err(Errno::EAGAIN) => break,
            Err(Errno::EINTR) => continue,
            Err(err) => {
                Logger::fatal(format!("Error reading inotify event(s): {err}"));
            },
        }
    }
}

//continue on error and log it to stderr
fn handle_signalfd(signal_fd: &mut SignalFd, inotify_state: &mut InotifyState, cfg: &mut Cfg) {
    loop{
        match signal_fd.read_signal() {
             Ok(signal) => match signal {
                Some(signal) => {
                    match Signal::try_from(signal.ssi_signo as i32) {
                        Ok(Signal::SIGHUP) => {
                            reload_cfg(inotify_state, cfg);     
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
             Err(err) => Logger::error(format!("Unable to read signal: {err}"))
        };

    }
}

fn handle_children(){
    loop{
        match waitpid(Pid::from_raw(-1), Some(WaitPidFlag::WNOHANG)){
            Ok(WaitStatus::StillAlive) => break, 
            Err(Errno::ECHILD) => break,
            _ => {}, 
        };

    }
}

fn reload_cfg(inotify_state: &mut InotifyState, cfg: &mut Cfg){

    match Cfg::init(&cfg.cli_config_path) {
        Ok(cfg_new) => *cfg = cfg_new,
        Err(err) => Logger::error(format!("Error parsing new config, reloading with the previous one: {err}"))
    }

    update_inotify(inotify_state, cfg);
}

fn update_inotify(inotify_state: &mut InotifyState, cfg: &Cfg) {
    inotify_state.del_watches();
    inotify_fill_from_cfg(inotify_state, cfg);
}

//continue on error and log it to stderr
fn inotify_fill_from_cfg(inotify_state: &mut InotifyState, cfg: &Cfg) {
    for entry in &cfg.entry {

        //cleanup symlinks as they will get traversed here upon resolving dir,
        //not true for FileType.is_dir() below as it does NOT traverse symlinks by default
        
        let extra = if entry.recursive && entry.path.is_dir() && !entry.path.is_symlink() {
            recursive_dir_walk(&entry.path).unwrap_or_default()
        } else {
            Vec::new()
        };

        let objs_to_watch = iter::once(entry.path.as_path())
            .chain(extra.iter().map(PathBuf::as_path));

        for obj in objs_to_watch {
            match inotify_state.inotify_fd.add_watch(obj, entry.events | AddWatchFlags::IN_DONT_FOLLOW) {
                Ok(wd) => {
                    inotify_state.wd_to_script.insert(wd, Rc::clone(&entry.script));
                }
                Err(err) => Logger::error(format!("Error adding watch for {obj:?}: {err}")),
            }
        }
    }
}

//not including the initial dir
//does NOT traverse symlinks
fn recursive_dir_walk(path: &Path) -> Option<Vec<PathBuf>> {

    let mut ret: Vec<PathBuf> = Vec::new();

    let path_iter = match path.read_dir(){
        Ok(iter) => iter,
        Err(err) => {
            Logger::error(format!("Failed to read {path:?}: {err}"));
            return None;
        }
    };

    for obj in path_iter{
        let obj = match obj {
            Ok(obj) => obj,
            Err(err) => { 
                Logger::error(format!("Failed to read {path:?}: {err}"));
                continue;
            }
        };

        if let Ok(filetype) = obj.file_type() {
            //is_dir does not resolve symlinks
            if filetype.is_dir() {
                let path = obj.path();
                ret.extend(recursive_dir_walk(&path).unwrap_or_default());
            }
        } else {
            Logger::error(format!("Failed to get filetype for {:?}", obj.path()));
            continue;
        }
    }
    Some(ret)
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
