use clap::Parser;
use std::path::PathBuf;


#[derive(Parser)]
#[command(version, about)]
pub struct CliArgs {
    
    ///Path to config
    #[arg(short, long)]
    pub config: Option<PathBuf>,
    
    ///Verbose mode
    #[arg(short, long)]
    pub verbose: bool,

    ///Check config, report to stdout and exit
    #[arg(short = 'r', long)]
    pub check_config: bool
}


