use nix::sys::inotify::AddWatchFlags;
mod config;
mod args;






fn main() {
    println!("{:?}", AddWatchFlags::from_name("IN_MODIFY"))
}

