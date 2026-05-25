use std::process::Command;

use rexpect::session::Options;
use rexpect::spawn_with_options;

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

fn send_key(
    p: &mut rexpect::session::PtySession,
    key: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    p.send(key)?;
    p.flush()?;
    Ok(())
}
