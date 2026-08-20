use serde::{Deserialize, Deserializer};
use std::path::PathBuf;
use std::fs::File;
use std::io::Read;
use nix::sys::inotify::AddWatchFlags;


#[derive(Deserialize)]
pub struct Cfg {
    #[serde(rename = "watch")]
    entry: Vec<CfgEntry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CfgEntry {
    pub path: PathBuf,
    pub recursive: Option<bool>,
    pub script: PathBuf,

    #[serde(deserialize_with = "deserialize_events")]
    pub events: AddWatchFlags,
}

//shit code, rewrite after impl cli flags
impl Cfg {
    pub fn init() -> Self {
        let mut configs: Vec<PathBuf> = default_cfg_paths();
        let config_in_use = configs.pop().unwrap_or(PathBuf::from("/etc/fchekd/config.toml"));

        let mut fd = File::open(config_in_use).expect("failed to open config");
        let mut cfg_contents = String::new();
        fd.read_to_string(&mut cfg_contents).expect("failed to read contets");
        
        toml::from_str(&cfg_contents).expect("error parsing")

        }
    }


//as FILO, don't really need two paths in mem, think of smth better later
//also expect() placeholders, either propagate or better kms on error
//
fn default_cfg_paths() -> Vec<PathBuf> {
    let mut retpaths = Vec::new();
    
    let defconf_path = "/etc/fcheckd/config.toml"; //default via unwrap_or on Cfg.init() anyways
    let defconf_home_path = ".config/fchekd/config.toml";

    retpaths.push(PathBuf::from(defconf_path));
    match std::env::var_os("HOME") {
        Some(home_path) => retpaths.push(PathBuf::from(home_path).join(defconf_home_path)),
        None => {},
    }
    retpaths
}


fn deserialize_events<'de, D>(deserializer: D) -> Result<AddWatchFlags, D::Error>
where
    D: Deserializer<'de>,
{
    let names: Vec<String> = Vec::deserialize(deserializer)?;
    let mut mask = AddWatchFlags::empty();

    for name in names {
        let upper = format!("IN_{}", name.to_uppercase());               
        match AddWatchFlags::from_name(&upper) {
            Some(flag) => mask |= flag,
            None => return Err(serde::de::Error::custom(format!("unknown inotify event: {name}"))),
        }
    }

    Ok(mask)
}

