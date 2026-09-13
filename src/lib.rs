// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 subparr <subparr@tuta.io>
 
//child processess are not waited on through handle which Command::spawn()
//returns but rather through manual waitpid() syscall
//to not block the thread
//so the warning gets disregarded
#![allow(clippy::zombie_processes)]
 
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
 
pub mod config;
pub mod logger;
 
use config::Cfg;
use logger::Logger;
 
pub const INOTIFY_EPOLL_TOKEN: u64 = 0;
pub const SIGNAL_EPOLL_TOKEN: u64 = 1;
pub const EPOLL_BUF_LEN: usize = 2;

//wd_to_script may contain invalid entries after move/delete/umount
//but events will not be registered for those files after the invalidation
//see  https://man7.org/linux/man-pages/man7/inotify.7.html
//any pending events before invalidation are available 
//and WILL be processed
pub struct InotifyState {
    pub inotify_fd: Inotify,
    pub wd_to_script: HashMap<WatchDescriptor, Rc<PathBuf>>,
}

impl InotifyState {
    pub fn del_watches(&mut self) {
        for (wd, _) in self.wd_to_script.drain() {
                self.inotify_fd.rm_watch(wd).unwrap_or_else(|err| {
                    Logger::error(format!("Error removing watch: {err}")); //EINVAL, can happen on IN_IGNORED
                });
            }
    }

}


pub fn event_loop(mut inotify_state: InotifyState, mut signal_fd: SignalFd, epoll_fd: Epoll, mut cfg: Cfg) { 
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


pub fn handle_inotify(inotify_state: &InotifyState) {
    loop{
        match inotify_state.inotify_fd.read_events(){
            Ok(events) => {
                for event in events {
                    //IN_ONESHOT caused script to fire twice because existing 
                    //wd_to_script entry, ignore signal omitted by rm_watch
                    if event.mask.contains(AddWatchFlags::IN_IGNORED) {
                        Logger::error(format!("IN_IGNORE received on {event:?}! File will not be watched.")); 
                        continue;
                    }
                    if event.mask.contains(AddWatchFlags::IN_Q_OVERFLOW) {
                       Logger::error("Inotify event queue is overflowed. Some events may be dropped"); 
                    }
                    
                    let script = match inotify_state.wd_to_script.get(&event.wd){
                        Some(script) => script,
                        None => {
                            Logger::error("No script path found for inotify wd. Skipping");
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
                    
                    Logger::info(format!("Received event, starting script {script:?}"));

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
pub fn handle_signalfd(signal_fd: &mut SignalFd, inotify_state: &mut InotifyState, cfg: &mut Cfg) {
    loop{
        match signal_fd.read_signal() {
             Ok(signal) => match signal {
                Some(signal) => {
                    match Signal::try_from(signal.ssi_signo as i32) {
                        Ok(Signal::SIGHUP) => {
                            Logger::info("Received SIGHUP, reloading config");
                            reload_cfg(inotify_state, cfg);
                        }
                        Ok(Signal::SIGCHLD) => {
                            Logger::info("Received SIGCHLD, reaping child");
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

pub fn handle_children(){
    loop{
        match waitpid(Pid::from_raw(-1), Some(WaitPidFlag::WNOHANG)){
            Ok(WaitStatus::StillAlive) => break, 
            Err(Errno::ECHILD) => break,
            _ => {}, 
        };

    }
}

pub fn reload_cfg(inotify_state: &mut InotifyState, cfg: &mut Cfg){

    match Cfg::init(&cfg.cli_config_path) {
        Ok(cfg_new) => *cfg = cfg_new,
        Err(err) => Logger::error(format!("Error parsing new config, reloading with the previous one: {err}"))
    }

    update_inotify(inotify_state, cfg);
}

pub fn update_inotify(inotify_state: &mut InotifyState, cfg: &Cfg) {
    inotify_state.del_watches();
    inotify_fill_from_cfg(inotify_state, cfg);
}

//continue on error and log it to stderr
pub fn inotify_fill_from_cfg(inotify_state: &mut InotifyState, cfg: &Cfg) {
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
                    Logger::info(format!("Added watch for {:?}", obj));
                }
                Err(err) => Logger::error(format!("Error adding watch for {obj:?}: {err}")),
            }
        }
    }
}

//not including the initial dir
pub fn recursive_dir_walk(path: &Path) -> Option<Vec<PathBuf>> {

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
                ret.push(path);
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
pub fn init_inotify(cfg: &Cfg) -> Result<InotifyState, Errno> { 
    let mut inotify_state = InotifyState{
        inotify_fd: Inotify::init(InitFlags::IN_CLOEXEC | InitFlags::IN_NONBLOCK)?,
        wd_to_script: HashMap::new(),
    };

    inotify_fill_from_cfg(&mut inotify_state, cfg);
    Ok(inotify_state)
}


pub fn init_epoll(inotify_state: &InotifyState, signal_fd: &SignalFd) -> Result<Epoll, Errno>{

    let epoll_fd = Epoll::new(EpollCreateFlags::EPOLL_CLOEXEC)?;

    epoll_fd.add(&inotify_state.inotify_fd, EpollEvent::new(EpollFlags::EPOLLIN, INOTIFY_EPOLL_TOKEN))?;
    epoll_fd.add(signal_fd, EpollEvent::new(EpollFlags::EPOLLIN, SIGNAL_EPOLL_TOKEN))?;

    Ok(epoll_fd)
}


pub fn init_signals() -> Result<SignalFd, Errno> {
    let mask = Signal::SIGHUP | Signal::SIGCHLD;
    sigprocmask(SigmaskHow::SIG_BLOCK, Some(&mask), None)?;
    
    let signal_fd = SignalFd::with_flags(&mask, SfdFlags::SFD_CLOEXEC | SfdFlags::SFD_NONBLOCK)?;
    Ok(signal_fd)
}


//here are tests mostly for checking the recursion
//assume all functions names are true as in what they do
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;
 
    //always init logger in tests as once the tested code tries to call it
    //it will panic because the logger was not initialized
    //also inits once across all of them so that it doesn't panic
    //because been inited 2nd+ time
    fn init_test_logger() {
        static INIT: std::sync::Once = std::sync::Once::new();
        INIT.call_once(|| {
            Logger::init(false, &None);
        });
    }
 
    #[test]
    fn empty_dir_returns_empty_vec() {
        init_test_logger();
        let dir = tempdir().unwrap();
        let result = recursive_dir_walk(dir.path()).unwrap();
        assert!(result.is_empty());
    }
 
    #[test]
    fn nonexistent_path_returns_none() {
        init_test_logger();
        let result = recursive_dir_walk(Path::new("/jhlkasdghasdgfkjha/asdgikahsdgkljasdg/pqoruoiqwpro/oooooooo"));
        assert!(result.is_none());
    }
 
    #[test]
    fn finds_nested_subdirs() {
        init_test_logger();
        let dir = tempdir().unwrap();
        let a = dir.path().join("a");
        let b = a.join("b");
        fs::create_dir(&a).unwrap();
        fs::create_dir(&b).unwrap();
        fs::write(a.join("file"), b"hjlk").unwrap();
 
        let mut result = recursive_dir_walk(dir.path()).unwrap();
        result.sort();
        let mut expected = vec![a.clone(), b.clone()];
        expected.sort();
        assert_eq!(result, expected);
    }
 
    #[test]
    fn does_not_include_root_dir() {
        init_test_logger();
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join("child")).unwrap();
        let result = recursive_dir_walk(dir.path()).unwrap();
        assert!(!result.contains(&dir.path().to_path_buf()));
    }
 
    #[test]
    fn siblings_and_multi_level_nesting_no_duplicates() {
        init_test_logger();
        let dir = tempdir().unwrap();
        let a = dir.path().join("a");
        let b = a.join("b");
        let c = a.join("c");
        let d = dir.path().join("d");
        fs::create_dir_all(&b).unwrap();
        fs::create_dir_all(&c).unwrap();
        fs::create_dir_all(&d).unwrap();
 
        let mut result = recursive_dir_walk(dir.path()).unwrap();
        let before_len = result.len();
        result.sort();
        result.dedup();
        let after_dedup_len = result.len();
        assert_eq!(before_len, after_dedup_len, "duplicates found: {:?}", result);
 
        let mut expected = vec![a, b, c, d];
        expected.sort();
        assert_eq!(result, expected);
    }
 
    #[test]
    fn three_levels_deep_all_recorded() {
        init_test_logger();
        let dir = tempdir().unwrap();
        let a = dir.path().join("a");
        let b = a.join("b");
        let c = b.join("c");
        fs::create_dir_all(&c).unwrap();
 
        let mut result = recursive_dir_walk(dir.path()).unwrap();
        result.sort();
        let mut expected = vec![a, b, c];
        expected.sort();
        assert_eq!(result, expected);
    }

    #[test]
    fn handle_children_reaps_exited_child() {
        //look out for waitpid(-1,..) in handle_children as it may kill other children
        init_test_logger();
 
        let child = std::process::Command::new("/bin/true")
            .spawn()
            .expect("failed to spawn /bin/true");
        let pid = child.id();
 
        std::thread::sleep(std::time::Duration::from_millis(100));

        handle_children();
 
        //try waiting on child pid, error is expected as it should've been reaped by
        //handle_choldren
        match waitpid(Pid::from_raw(pid as i32), Some(WaitPidFlag::WNOHANG)) {
            Err(Errno::ECHILD) => {} 
            other => panic!("{other:?}"),
        }
    }
}

