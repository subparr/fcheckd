use clap::Parser;
use std::ffi::OsString;


#[derive(Parser)]
#[command(version, about)]
pub struct CliArgs {
    
    #[arg(short, long)]
    pub config: OsString,
    
    #[arg(short, long)]
    pub verbose: bool,

    #[arg(short, long)]
    pub check_config: bool
}


