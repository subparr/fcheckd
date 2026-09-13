// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 subparr <subparr@tuta.io>

use fcheckd::config::Cfg;
use fcheckd::{init_inotify, recursive_dir_walk, reload_cfg};

use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::time::Duration;

static LOG_PATH: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

fn init_logger_once() -> &'static PathBuf {
    static INIT: std::sync::Once = std::sync::Once::new();
    INIT.call_once(|| {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fcheckd-test.log");
        //leak the tempdir as it needs to outlive every test
        //and this process exits right after the test run anyways
        std::mem::forget(dir);
        fcheckd::logger::Logger::init(true, &Some(path.clone()));
        LOG_PATH.set(path).unwrap();
    });
    LOG_PATH.get().unwrap()
}

fn make_executable_script(path: &std::path::Path) {
    let mut f = fs::File::create(path).unwrap();
    writeln!(f, "#!/bin/sh\nexit 0").unwrap();
    let mut perms = f.metadata().unwrap().permissions();
    perms.set_mode(0o755);
    f.set_permissions(perms).unwrap();
}

#[test]
fn recursive_config_watches_every_nested_dir() {
    init_logger_once();
    let dir = tempfile::tempdir().unwrap();
    let watched_root = dir.path().join("watched");
    let sub_a = watched_root.join("a");
    let sub_b = sub_a.join("b");
    fs::create_dir_all(&sub_b).unwrap();

    let script_path = dir.path().join("script.sh");
    make_executable_script(&script_path);

    let cfg_path = dir.path().join("config.toml");
    let cfg_contents = format!(
        r#"
        [[watch]]
        path = {watched_root:?}
        recursive = true
        script = {script_path:?}
        events = ["create"]
        "#
    );
    fs::write(&cfg_path, cfg_contents).unwrap();

    let cfg = Cfg::init(&Some(cfg_path)).expect("config should parse and validate");
    let inotify_state = init_inotify(&cfg).expect("inotify init should succeed");

    assert_eq!(
        inotify_state.wd_to_script.len(),
        3,
        "expected a watch on the root plus every nested subdir"
    );
}

#[test]
fn create_event_is_observed_and_maps_to_configured_script() {
    init_logger_once();
    let dir = tempfile::tempdir().unwrap();
    let watched = dir.path().join("watched");
    fs::create_dir(&watched).unwrap();

    let script_path = dir.path().join("script.sh");
    make_executable_script(&script_path);

    let cfg_path = dir.path().join("config.toml");
    let cfg_contents = format!(
        r#"
        [[watch]]
        path = {watched:?}
        script = {script_path:?}
        events = ["create"]
        "#
    );
    fs::write(&cfg_path, cfg_contents).unwrap();

    let cfg = Cfg::init(&Some(cfg_path)).expect("config should parse and validate");
    let inotify_state = init_inotify(&cfg).expect("inotify init should succeed");

    fs::write(watched.join("new_file.txt"), b"hi").unwrap();

    std::thread::sleep(Duration::from_millis(100));

    let events = inotify_state
        .inotify_fd
        .read_events()
        .expect("should be able to read the queued event");
    assert!(!events.is_empty(), "expected at least one inotify event");

    let event = &events[0];
    let mapped_script = inotify_state
        .wd_to_script
        .get(&event.wd)
        .expect("watch descriptor should map back to the configured script");
    assert_eq!(mapped_script.as_path(), script_path);
}

#[test]
fn recursive_dir_walk_matches_what_init_inotify_actually_watches() {
    init_logger_once();
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a");
    let b = a.join("b");
    fs::create_dir_all(&b).unwrap();

    let found = recursive_dir_walk(dir.path()).unwrap();
    assert_eq!(found.len(), 2, "expected both a/ and a/b/ to be found");
}

#[test]
fn multiple_entries_each_get_their_own_watch() {
    init_logger_once();
    let dir = tempfile::tempdir().unwrap();
    let dir_a = dir.path().join("a");
    let dir_b = dir.path().join("b");
    fs::create_dir(&dir_a).unwrap();
    fs::create_dir(&dir_b).unwrap();

    let script_path = dir.path().join("script.sh");
    make_executable_script(&script_path);

    let cfg_path = dir.path().join("config.toml");
    let cfg_contents = format!(
        r#"
        [[watch]]
        path = {dir_a:?}
        script = {script_path:?}
        events = ["create"]

        [[watch]]
        path = {dir_b:?}
        script = {script_path:?}
        events = ["delete"]
        "#
    );
    fs::write(&cfg_path, cfg_contents).unwrap();

    let cfg = Cfg::init(&Some(cfg_path)).expect("config should parse");
    let inotify_state = init_inotify(&cfg).expect("inotify init should succeed");
    assert_eq!(inotify_state.wd_to_script.len(), 2);
}

#[test]
fn vanished_path_fails_the_watch_without_panicking_and_logs_it() {
    //toctou on Cfg::init validates path existsence, but nothing stops it for disappearing
    let log_path = init_logger_once();
    let dir = tempfile::tempdir().unwrap();
    let watched = dir.path().join("will_vanish");
    fs::create_dir(&watched).unwrap();

    let script_path = dir.path().join("script.sh");
    make_executable_script(&script_path);

    let cfg_path = dir.path().join("config.toml");
    let cfg_contents = format!(
        r#"
        [[watch]]
        path = {watched:?}
        script = {script_path:?}
        events = ["create"]
        "#
    );
    fs::write(&cfg_path, cfg_contents).unwrap();

    //exists right now, so this passes validation
    let cfg = Cfg::init(&Some(cfg_path)).expect("config should parse and validate");

    //and now it doesn't
    fs::remove_dir(&watched).unwrap();

    let inotify_state = init_inotify(&cfg).expect("inotify init itself must not fail");
    assert_eq!(
        inotify_state.wd_to_script.len(),
        0,
        "watch on a vanished path should fail, not silently succeed"
    );

    let log_contents = fs::read_to_string(log_path).unwrap();
    assert!(
        log_contents.contains("Error adding watch"),
        "expected the add_watch failure to be logged, got: {log_contents}"
    );
}

#[test]
fn reload_cfg_picks_up_new_entries_and_rebuilds_watches() {
    init_logger_once();
    let dir = tempfile::tempdir().unwrap();
    let dir_a = dir.path().join("a");
    let dir_b = dir.path().join("b");
    fs::create_dir(&dir_a).unwrap();
    fs::create_dir(&dir_b).unwrap();

    let script_path = dir.path().join("script.sh");
    make_executable_script(&script_path);

    let cfg_path = dir.path().join("config.toml");
    let initial = format!(
        r#"
        [[watch]]
        path = {dir_a:?}
        script = {script_path:?}
        events = ["create"]
        "#
    );
    fs::write(&cfg_path, &initial).unwrap();

    let mut cfg = Cfg::init(&Some(cfg_path.clone())).unwrap();
    let mut inotify_state = init_inotify(&cfg).unwrap();
    assert_eq!(inotify_state.wd_to_script.len(), 1);

    let updated = format!(
        r#"
        [[watch]]
        path = {dir_a:?}
        script = {script_path:?}
        events = ["create"]

        [[watch]]
        path = {dir_b:?}
        script = {script_path:?}
        events = ["delete"]
        "#
    );
    fs::write(&cfg_path, &updated).unwrap();

    reload_cfg(&mut inotify_state, &mut cfg);

    assert_eq!(cfg.entry.len(), 2, "cfg should reflect the new entry");
    assert_eq!(inotify_state.wd_to_script.len(), 2, "watches should be rebuilt from the new cfg");
}

