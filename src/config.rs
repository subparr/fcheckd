use serde::{Deserialize, Deserializer};
use std::path::{Path, PathBuf};
use std::fs::File;
use std::io::Read;
use nix::sys::inotify::AddWatchFlags;

//consume args, store the needed -c value if exists
//rewrite to use OpenOptions to create default cfg under /etc + a dir

#[derive(Deserialize)]
pub struct Cfg {
    #[serde(rename = "watch")]
    pub entry: Vec<CfgEntry>,
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

//fallback to default on cfg unavailability, not only based on hierarchy, ie check every one and
//handle cfg read in config module function
impl Cfg {
    pub fn init(cli_config_path: Option<&Path>) -> Result<Self, Box<dyn std::error::Error>> {
        let config_in_use = match cli_config_path {
            Some(path) => path.to_path_buf(),     
            None => {
                match home_config_path() {
                    Some(path) => path,
                    None => PathBuf::from("/etc/fcheckd/config.toml")
                }
            }
        };

        let mut fd = File::open(config_in_use)?;
        let mut cfg_contents = String::new();
        fd.read_to_string(&mut cfg_contents)?;
        
        Ok(toml::from_str(&cfg_contents)?)

        }
    }


fn home_config_path() -> Option<PathBuf> {
    let defconf_home_path = ".config/fcheckd/config.toml";
    match std::env::var_os("HOME") {
        Some(home_path) => Some(PathBuf::from(home_path).join(defconf_home_path)),
        None => None,
    }
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

