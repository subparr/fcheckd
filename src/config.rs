use serde::Deserialize;
use std::path::PathBuf;


#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CfgEntry {
    path: PathBuf,
    recursive: Option<bool>,
    script: PathBuf,
    events: u32,
}

//impl CfgEntry {
//    fn init() -> Self {
//        let cfgpaths: Vec<PathBuf> = ["/etc/fcheckd/config.toml","~/.config/fcheckd/config.toml"];
//    }
//}

#[allow(dead_code)]
fn default_cfg_paths() -> Vec<PathBuf> {
    let mut retpaths = Vec::new();
    
    let defconf_path = "/etc/fcheckd/config.toml";
    let defconf_home_path = ".config/fchekd/config.toml";

    //add vars for default tf is this
    match std::env::var_os("HOME") {
        Some(val) => retpaths.push(PathBuf::from(val).join(defconf_home_path)),
        None => {},
    }
    retpaths.push(PathBuf::from(defconf_path));

    retpaths
}
