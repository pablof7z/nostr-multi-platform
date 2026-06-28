use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use rexpect::session::Options;
use rexpect::spawn_with_options;

const TEST_NSEC: &str = "nsec1vl029mgpspedva04g90vltkh6fvh240zqtv9k0t9af8935ke9laqsnlfe5";

/// Smoke test: TUI boots, shows welcome screen (no account configured),
/// help overlay works on top of it, and exits cleanly on `q`.
#[test]
fn tui_boots_and_quits_cleanly() -> Result<(), Box<dyn std::error::Error>> {
    let bin = env!("CARGO_BIN_EXE_chirp-tui");
    let mut command = Command::new("sh");
    command.args([
        "-lc",
        "stty rows 40 cols 120; exec \"$CHIRP_TUI_BIN\" --relay wss://relay.damus.io",
    ]);
    command.env("CHIRP_TUI_BIN", bin);

    let mut p = spawn_with_options(command, Options::new().timeout_ms(Some(20_000)))?;
    p.process_mut().set_kill_timeout(Some(2_000));

    // Welcome screen shows app name and subtitle.
    p.exp_string("chirp")?;
    p.exp_string("nostr")?;

    // Help overlay still works on top of the welcome screen.
    send_key(&mut p, "?")?;
    p.exp_string("Help")?;
    send_key(&mut p, "?")?;

    send_key(&mut p, "q")?;
    let _ = p.exp_eof();
    Ok(())
}

#[test]
#[ignore = "live public-relay publish settlement is opt-in; deterministic coverage lives in published_outbox.rs"]
fn published_history_row_opens_detail() -> Result<(), Box<dyn std::error::Error>> {
    let bin = env!("CARGO_BIN_EXE_chirp-tui");
    let home = isolated_home("published-detail")?;
    let xdg = home.join(".local/share");
    std::fs::create_dir_all(&xdg)?;

    let mut command = Command::new("sh");
    command.args([
        "-lc",
        "stty rows 40 cols 120; exec \"$CHIRP_TUI_BIN\" --relay wss://relay.damus.io",
    ]);
    command.env("CHIRP_TUI_BIN", bin);
    command.env("HOME", &home);
    command.env("XDG_DATA_HOME", &xdg);

    let mut p = spawn_with_options(command, Options::new().timeout_ms(Some(90_000)))?;
    p.process_mut().set_kill_timeout(Some(2_000));

    p.exp_string("chirp")?;
    send_key(&mut p, "n")?;
    p.exp_string("nsec")?;
    send_key(&mut p, TEST_NSEC)?;
    send_enter(&mut p)?;
    p.exp_string("npub1")?;

    let note = format!("chirp-tui published detail e2e {}", now_millis());
    send_key(&mut p, "n")?;
    p.exp_string("New Note")?;
    send_key(&mut p, &note)?;
    send_enter(&mut p)?;
    send_key(&mut p, "s")?;
    p.exp_string("Outbox")?;
    p.exp_string("Published")?;
    send_enter(&mut p)?;
    p.exp_string("Published Detail")?;
    p.exp_string("event")?;
    p.exp_string("kind")?;
    p.exp_string("action")?;

    send_key(&mut p, "q")?;
    let _ = p.exp_eof();
    let _ = std::fs::remove_dir_all(home);
    Ok(())
}

fn send_key(
    p: &mut rexpect::session::PtySession,
    key: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    p.send(key)?;
    p.flush()?;
    Ok(())
}

fn send_enter(p: &mut rexpect::session::PtySession) -> Result<(), Box<dyn std::error::Error>> {
    send_key(p, "\r")
}

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default()
}

fn isolated_home(label: &str) -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    let path = std::env::temp_dir().join(format!("chirp-tui-{label}-{}", now_millis()));
    std::fs::create_dir_all(&path)?;
    Ok(path)
}
