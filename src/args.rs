use clap::Parser;
use std::path::PathBuf;


#[derive(Parser)]
#[command(version, about)]
pub struct CliArgs {
    
    ///Path to config
    #[arg(short, long)]
    pub config: Option<PathBuf>,

    ///Check config, report to stdout and exit
    #[arg(short = 'e', long)]
    pub check_config: bool,

    ///Make output more verbose
    #[arg(short = 'v', long)]
    pub verbose: bool,

    ///Write log to a file instead of stderr/stdout
    #[arg(short = 'l', long)]
    pub log_file: Option<PathBuf>,
}


