#![allow(dead_code)]
#![allow(unused_imports)]
#![allow(unused_variables)]

use nix::sys::inotify::AddWatchFlags;
mod config;
mod args;






fn main() {
    let cfg = config::Cfg::init();

}

