//! First-launch setup (PLAN-onboarding.md): once per profile, at the first
//! deliberate opening of the window, only the permissions the user must
//! allow are offered, and only an explicit action asks. Game access is the
//! row tests can drive on every OS: the guarded table and the user's
//! answers come from the test environment. The UI is a stand-in.

mod common;

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use common::*;
use serde_json::{Value, json};

struct Setup {
    world: World,
    log: PathBuf,
}

impl Setup {
    fn new() -> Setup {
        let world = World::new();
        let log = world.root.join("ui.log");
        Setup { world, log }
    }

    fn env(&self) -> Vec<(&'static str, String)> {
        vec![("SAVESCUMMER_UI_EXE", FAKE_UI.to_string()), ("FAKE_UI_LOG", self.log.to_string_lossy().into_owned())]
    }

    fn host(&self, launched: bool, args: &[&str]) -> HostProcess {
        let env = self.env();
        let env: Vec<(&str, &str)> = env.iter().map(|(k, v)| (*k, v.as_str())).collect();
        if launched { self.world.host_launched_with(args, &env) } else { self.world.host_with(args, &env) }
    }

    /// A Steam game saving under Documents, which the environment guards
    /// and where the user answers `answer`.
    fn guarded_game(&self, answer: &str) -> String {
        let world = &self.world;
        let mut catalog = fixture_catalog();
        catalog["games"].as_array_mut().unwrap().push(json!({
            "id": "steam-9001",
            "name": "Docs One",
            "detect": { "steam": 9001 },
            "installDirs": ["Docs One"],
            "executables": { "windows": ["DocsOne.exe"] },
            "save": [ { "path": "{DOCUMENTS}/Docs One" } ]
        }));
        world.set_catalog(&catalog);
        world.steam_install(9001, "Docs One", "DocsOne.exe");
        write(&world.documents.join("Docs One").join("slot.sav"), "progress");
        world.set_env("privacy", json!({ "folders": [[world.documents, "documents"]] }));
        world.set_env("privacy_answers", json!({ "documents": answer }));
        "steam-9001".into()
    }

    /// The setup screen once its rows are known.
    fn screen(&self) -> Value {
        self.world.wait_state("the setup rows", |s| s["onboarding"]["inspecting"] == false)["onboarding"].clone()
    }

    fn request(&self, session: &str, row: &str) -> Value {
        let line =
            json!({ "v": 2, "id": "request", "type": "request_onboarding_permission", "session": session, "row": row });
        self.world.cli(&["raw", &line.to_string()]).last()
    }

    fn finish(&self, session: &str) -> Value {
        let line = json!({ "v": 2, "id": "finish", "type": "finish_onboarding", "session": session });
        self.world.ok(&["raw", &line.to_string()])
    }

    fn sessions_started(&self) -> usize {
        self.world.host_log().matches("first launch: setting up").count()
    }
}

#[test]
fn nothing_to_allow_means_no_screen_and_no_second_chance() {
    let setup = Setup::new();
    let mut host = setup.host(true, &[]);
    wait_for("setup over", Duration::from_secs(20), || {
        setup.world.host_log().contains("first launch: nothing to ask").then_some(())
    });
    assert!(setup.world.state()["onboarding"].is_null());
    host.kill();
    let _host = setup.host(true, &[]);
    setup.world.ok(&["show-ui"]);
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(setup.sessions_started(), 1, "{}", setup.world.host_log());
}

#[test]
fn a_background_start_waits_for_the_window_and_asks_only_on_request() {
    let setup = Setup::new();
    let game = setup.guarded_game("granted");
    let mut host = setup.host(false, &[]);
    assert_eq!(setup.world.game(&game)["access"]["category"], "documents");
    assert!(setup.world.state()["onboarding"].is_null(), "a minimized start isn't the first opening");

    setup.world.ok(&["show-ui"]);
    let screen = setup.screen();
    assert_eq!(
        screen["rows"],
        json!([{ "id": "game_access", "kind": "game_access", "status": "needs_action", "action": "allow_access" }])
    );
    assert_eq!(screen["any_permission_confirmed"], false);
    assert!(!setup.world.host_log().contains("asking for access"), "nothing asked before the action");

    let session = s(&screen["session"]);
    assert_eq!(setup.request(&session, "game_access")["result"]["status"], "granted");
    let screen = setup.world.state()["onboarding"].clone();
    assert_eq!(screen["rows"][0]["status"], "granted");
    assert_eq!(screen["any_permission_confirmed"], true);
    assert!(setup.world.game(&game)["access"].is_null(), "the game is active now");

    // Requests outside the live session are refused; finishing twice is fine.
    assert_eq!(setup.request("onboarding-stale", "game_access")["error"]["kind"], "invalid_request");
    setup.finish(&session);
    assert!(setup.world.state()["onboarding"].is_null());
    assert_eq!(setup.request(&session, "game_access")["error"]["kind"], "invalid_request");
    setup.finish(&session);

    host.kill();
    let _host = setup.host(true, &[]);
    setup.world.ok(&["show-ui"]);
    std::thread::sleep(Duration::from_millis(500));
    assert!(setup.world.state()["onboarding"].is_null());
    assert_eq!(setup.sessions_started(), 1);
}

#[test]
fn a_denial_keeps_skip_and_offers_the_settings_pane() {
    let setup = Setup::new();
    setup.guarded_game("denied");
    let _host = setup.host(true, &[]);
    let session = s(&setup.screen()["session"]);
    assert_eq!(setup.request(&session, "game_access")["result"]["status"], "denied");
    let row = setup.world.state()["onboarding"]["rows"][0].clone();
    assert_eq!(row["action"], "open_settings");
    assert!(row["message"].is_string());
    assert_eq!(setup.world.state()["onboarding"]["any_permission_confirmed"], false);
}

#[test]
fn closing_the_window_finishes_setup() {
    let setup = Setup::new();
    setup.guarded_game("granted");
    let mut host = setup.host(true, &["--onboarding-grace-secs", "1"]);
    setup.screen();
    wait_for("the UI connects", Duration::from_secs(20), || {
        std::fs::read_to_string(&setup.log).unwrap_or_default().contains("connected").then_some(())
    });
    close_window(&setup.log);
    setup.world.wait_state("setup over", |s| s["onboarding"].is_null());
    assert!(!setup.world.host_log().contains("asking for access"));
    host.kill();
    let _host = setup.host(true, &[]);
    std::thread::sleep(Duration::from_millis(1500));
    assert!(setup.world.state()["onboarding"].is_null());
    assert_eq!(setup.sessions_started(), 1);
}

#[test]
fn a_host_that_dies_during_setup_never_starts_it_again() {
    let setup = Setup::new();
    setup.guarded_game("granted");
    let mut host = setup.host(false, &[]);
    setup.world.ok(&["show-ui"]);
    setup.screen();
    host.kill();
    let _host = setup.host(true, &[]);
    setup.world.ok(&["show-ui"]);
    std::thread::sleep(Duration::from_millis(1500));
    assert!(setup.world.state()["onboarding"].is_null());
    assert_eq!(setup.sessions_started(), 1, "{}", setup.world.host_log());
}

#[test]
fn the_installer_flag_records_its_choice_without_a_run_or_setup() {
    let setup = Setup::new();
    setup.guarded_game("granted");
    let data = setup.world.data.to_string_lossy().into_owned();
    let flag = Command::new(HOST)
        .args(["--data-dir", &data, "--autostart", "off"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let out = output_within(flag, CLI_LIMIT, "--autostart off");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stdout));
    assert!(String::from_utf8_lossy(&out.stdout).contains("start at login: off"));
    assert!(setup.world.data.join("host.db").is_file(), "the choice is recorded");

    let _host = setup.host(false, &[]);
    assert_eq!(setup.world.ok(&["host-runs"])["runs"].as_array().unwrap().len(), 1, "the flag isn't a run");
    // First-launch setup is still ahead.
    setup.world.ok(&["show-ui"]);
    assert_eq!(setup.screen()["rows"][0]["kind"], "game_access");
}

/// Ends the stand-in UI that logged to `log`, as closing the window does.
fn close_window(log: &Path) {
    let text = std::fs::read_to_string(log).unwrap_or_default();
    let pid = text.lines().find_map(|l| l.strip_prefix("pid ")).expect("the UI's pid").to_string();
    #[cfg(unix)]
    let status = Command::new("kill").arg("-9").arg(&pid).status();
    #[cfg(windows)]
    let status = Command::new("taskkill").args(["/F", "/PID", &pid]).status();
    assert!(status.unwrap().success());
}
