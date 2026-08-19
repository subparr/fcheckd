use serde::{Deserialize, Deserializer};
use std::path::PathBuf;
use std::fs::File;
use nix::sys::inotify::AddWatchFlags;

#[derive(Deserialize)]
struct Cfg {
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

//pub impl Cfg {
//    fn init() -> Result<Self, String> {
//    }
//}



#[allow(dead_code)]
fn default_cfg_paths() -> Vec<PathBuf> {
    let mut retpaths = Vec::new();
    
    let defconf_path = "/etc/fcheckd/config.toml";
    let defconf_home_path = ".config/fchekd/config.toml";

    match std::env::var_os("HOME") {
        Some(home_path) => retpaths.push(PathBuf::from(home_path).join(defconf_home_path)),
        None => {},
    }
    retpaths.push(PathBuf::from(defconf_path));
    retpaths
}

fn deserialize_events<'de, D>(deserializer: D) -> Result<AddWatchFlags, D::Error>
where
    D: Deserializer<'de>,
{
    let names: Vec<String> = Vec::deserialize(deserializer)?;
    let mut mask = AddWatchFlags::empty();

    for name in names {
        let upper = format!("IN_{}", name.to_uppercase());               //check if IN_upper + dir
        match AddWatchFlags::from_name(&upper) {
            Some(flag) => mask |= flag,
            None => return Err(serde::de::Error::custom(format!("unknown inotify event: {name}"))),
        }
    }

    Ok(mask)
}

