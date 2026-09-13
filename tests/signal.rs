// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 subparr <subparr@tuta.io>
//
//tests everything related to signals, handle_children is tested
//separately as a unit test

use fcheckd::config::Cfg;
use fcheckd::{handle_signalfd, init_inotify, init_signals};

use nix::sys::signal::{raise, Signal};
use std::fs;

fn init_test_logger() {
    static INIT: std::sync::Once = std::sync::Once::new();
    INIT.call_once(|| {
        fcheckd::logger::Logger::init(false, &None);
    });
}

#[test]
fn sighup_dispatches_to_reload_cfg_via_handle_signalfd() {
    init_test_logger();
    let dir = tempfile::tempdir().unwrap();
    let watched = dir.path().join("watched");
    fs::create_dir(&watched).unwrap();

    let script_path = dir.path().join("script.sh");
    fs::write(&script_path, "#!/bin/sh\nexit 0\n").unwrap();
    let mut perms = fs::metadata(&script_path).unwrap().permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut perms, 0o755);
    fs::set_permissions(&script_path, perms).unwrap();

    let cfg_path = dir.path().join("config.toml");
    fs::write(
        &cfg_path,
        format!(
            r#"
            [[watch]]
            path = {watched:?}
            script = {script_path:?}
            events = ["create"]
            "#
        ),
    )
    .unwrap();

    let mut cfg = Cfg::init(&Some(cfg_path.clone())).unwrap();
    let mut inotify_state = init_inotify(&cfg).unwrap();
    assert_eq!(cfg.entry.len(), 1);

    let watched_2 = dir.path().join("watched_2");
    fs::create_dir(&watched_2).unwrap();
    fs::write(
        &cfg_path,
        format!(
            r#"
            [[watch]]
            path = {watched:?}
            script = {script_path:?}
            events = ["create"]

            [[watch]]
            path = {watched_2:?}
            script = {script_path:?}
            events = ["delete"]
            "#
        ),
    )
    .unwrap();

    let mut signal_fd = init_signals().expect("init_signals should succeed");
    raise(Signal::SIGHUP).expect("raise SIGHUP should succeed");

    handle_signalfd(&mut signal_fd, &mut inotify_state, &mut cfg);

    assert_eq!(cfg.entry.len(), 2, "reload_cfg should've picked up new entry via SIGHUP");
    assert_eq!(inotify_state.wd_to_script.len(), 2, "watches should reflect reloaded config");
}
