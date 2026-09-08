// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 subparr <subparr@tuta.io>

use std::process;
use std::fmt::Display;
use std::sync::OnceLock;
use std::path::PathBuf;
use std::fs::File;
use std::fs::OpenOptions;
use std::io::Write;
use chrono::Local;

//DO create a log file if non existent
//parent directories will NOT be created

static LOGGER: OnceLock<Logger> = OnceLock::new();

#[derive(Debug)]
pub struct Logger {
    verbose: bool,
    file_log_fd: Option<File>,
}

impl Logger {
    pub fn init(verbose: bool, file_log_path: Option<PathBuf>) {

        let file_log_fd = file_log_path.map(|path| {
            OpenOptions::new().append(true).create(true).open(path)
            .unwrap_or_else(|err| {
                eprintln!("Failed opening/creating log file: {err}");
                process::exit(1);
            })
        });

        LOGGER.set(Self {verbose, file_log_fd}).expect("Logger already initialised");
    }
    
    pub fn fatal(message: impl Display) -> ! {
        Self::instance().write(true, message);
        process::exit(1);
    }

    pub fn error(message: impl Display) {
        Self::instance().write(true, message);
    }

    pub fn info(message: impl Display){
        let logger = Self::instance();
        if logger.verbose {
            Self::instance().write(false, message);
        }
    }
    
    //internal
    
    fn instance() -> &'static Logger {
        LOGGER.get().expect("Logger not initialised")
    }

    fn write(&self, to_stderr: bool, message: impl Display) {
        match &self.file_log_fd{
            Some(fd) => {
                 writeln!(&*fd, "[{}] {message}", Self::timestamp()); //hacky
            },
            None => {
                 if to_stderr{
                     eprintln!("{message}");
                 } else {
                     println!("{message}");
                 }
            },
        }
    }

    fn timestamp() -> String{
        Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
    }
}
