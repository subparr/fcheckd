use std::process;
use std::fmt::Display;
use std::sync::OnceLock;

//timestamps
//OnceLock or LazyLock impl

pub struct Logger {
    verbose: bool,
}

impl Logger {
    pub fn init(verbose: bool) -> Self{
       Self {
           verbose
       }
    }
    
    pub fn fatal(&self, message: impl Display){
        eprintln!("{message}");
        process::exit(1);
    }

    pub fn error(&self, message: impl Display){
        eprintln!("{message}");
    }

    pub fn info(&self, message: impl Display){
        if self.verbose {
            println!("{message}");
        }
    }

}
