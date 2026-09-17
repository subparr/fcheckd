// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 subparr <subparr@tuta.io>

use nix::sys::inotify::AddWatchFlags;
use serde::{Deserialize, Deserializer};
use std::fs::OpenOptions;
use std::io::Read;
use std::path::PathBuf;
use std::rc::Rc;

//Create default cfg paths with install script or systemd unit file opt
//not a job for the daemon itself as complicates the logic + possible permission problems
//on non_existent path report to stderr and exit

#[derive(Deserialize)]
pub struct Cfg {
    #[serde(rename = "watch")]
    pub entry: Vec<CfgEntry>,
    #[serde(skip)]
    pub cli_config_path: Option<PathBuf>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CfgEntry {
    #[serde(deserialize_with = "deserialize_check_path")]
    pub path: PathBuf,

    #[serde(default)]
    pub recursive: bool,

    #[serde(deserialize_with = "deserialize_check_script")]
    pub script: Rc<PathBuf>,

    #[serde(deserialize_with = "deserialize_events")]
    pub events: AddWatchFlags,
}

impl Cfg {
    pub fn init(cli_config_path: &Option<PathBuf>) -> Result<Self, Box<dyn std::error::Error>> {
        let config_in_use = match cli_config_path {
            Some(path) => path,
            None => &match home_config_path() {
                Some(path) => path,
                None => PathBuf::from("/etc/fcheckd/config.toml"),
            },
        };

        let mut fd = OpenOptions::new().read(true).open(config_in_use)?;
        let mut cfg_contents = String::new();
        fd.read_to_string(&mut cfg_contents)?;

        let mut cfg_instance: Cfg = toml::from_str(&cfg_contents)?;
        cfg_instance.cli_config_path = cli_config_path.clone(); //ew, but it's cheap
        Ok(cfg_instance)
    }
}

fn home_config_path() -> Option<PathBuf> {
    let defconf_home_path = ".config/fcheckd/config.toml";
    std::env::var_os("HOME").map(|home_path| PathBuf::from(home_path).join(defconf_home_path))
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
            None => {
                return Err(serde::de::Error::custom(format!(
                    "Unknown inotify event: {name}"
                )));
            }
        }
    }

    Ok(mask)
}

//enfore absolute existing scripts on cfg validation
fn deserialize_check_script<'de, D>(deserializer: D) -> Result<Rc<PathBuf>, D::Error>
where
    D: Deserializer<'de>,
{
    let script = PathBuf::deserialize(deserializer)?;

    if !script.is_absolute() {
        return Err(serde::de::Error::custom(format!(
            "Path must be absolute: {script:?}"
        )));
    }

    //resolving path if it is a symlink for initial add_watch,
    //further symlinks under the dir (in case watched object IS a dir)
    //will not be resolved intentionally i.e. IN_NO_FOLLOW inotify flag
    let script = match script.canonicalize() {
        Ok(script) => script,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Err(serde::de::Error::custom(format!(
                "Script does not exist: {script:?}"
            )));
        }
        Err(err) => {
            return Err(serde::de::Error::custom(format!(
                "Failed to canonicalize path {script:?}: {err}"
            )));
        }
    }; //handle perm denied?

    Ok(Rc::new(script))
}

fn deserialize_check_path<'de, D>(deserializer: D) -> Result<PathBuf, D::Error>
where
    D: Deserializer<'de>,
{
    let path = PathBuf::deserialize(deserializer)?;

    if !path.is_absolute() {
        return Err(serde::de::Error::custom(format!(
            "Path must be absolute: {path:?}"
        )));
    }

    let path = match path.canonicalize() {
        Ok(path) => path,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Err(serde::de::Error::custom(format!(
                "Watched object does not exist: {path:?}"
            )));
        }
        Err(err) => {
            return Err(serde::de::Error::custom(format!(
                "Failed to canonicalize path {path:?}: {err}"
            )));
        }
    };

    Ok(path)
}
