use std::process::Command;

use rexpect::session::Options;
use rexpect::spawn_with_options;

#[test]
fn tui_exposes_ios_parity_tabs_and_command_mode() -> Result<(), Box<dyn std::error::Error>> {
    let bin = env!("CARGO_BIN_EXE_chirp-tui");
    let mut command = Command::new("sh");
    command.args([
        "-lc",
        "stty rows 40 cols 120; exec \"$CHIRP_TUI_BIN\" --relay wss://relay.damus.io",
    ]);
    command.env("CHIRP_TUI_BIN", bin);

    let mut p = spawn_with_options(command, Options::new().timeout_ms(Some(20_000)))?;
    p.process_mut().set_kill_timeout(Some(2_000));

    p.exp_string("runtime")?;

    send_key(&mut p, "c")?;
    p.exp_string("Chats")?;
    send_key(&mut p, "g")?;
    p.exp_string("Groups")?;
    send_key(&mut p, "w")?;
    p.exp_string("Wallet")?;
    send_key(&mut p, "s")?;
    p.exp_string("Accounts")?;
    send_key(&mut p, "h")?;
    p.exp_string("Relays")?;

    send_key(&mut p, ":")?;
    p.exp_string("command mode")?;
    send_key(&mut p, "help\r")?;
    p.exp_string("dm-relays wallet")?;

    send_key(&mut p, "q")?;
    let _ = p.exp_eof();
    Ok(())
}

#[test]
fn tui_renders_fixture_mentions_and_media_labels() -> Result<(), Box<dyn std::error::Error>> {
    let bin = env!("CARGO_BIN_EXE_chirp-tui");
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/rich_renderer_snapshot.json");
    let mut command = Command::new("sh");
    command.args([
        "-lc",
        "stty rows 40 cols 120; exec \"$CHIRP_TUI_BIN\" --fixture-snapshot \"$FIXTURE\"",
    ]);
    command.env("CHIRP_TUI_BIN", bin);
    command.env("FIXTURE", fixture);

    let mut p = spawn_with_options(command, Options::new().timeout_ms(Some(10_000)))?;
    p.process_mut().set_kill_timeout(Some(2_000));

    p.exp_string("@branie")?;
    p.exp_string("[image]")?;

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
