use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::{AppRuntime, AppState};

pub(super) fn handle_input_bar_key(state: &mut AppState, runtime: &AppRuntime, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => state.cancel_input(),
        KeyCode::Backspace => state.backspace_input(),
        KeyCode::Enter => {
            if let Some((action, value)) = state.take_input() {
                dispatch_input_bar_action(&action, &value, state, runtime);
            }
        }
        KeyCode::Char(ch) if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT => {
            state.push_input_char(ch);
        }
        _ => {}
    }
}

fn dispatch_input_bar_action(
    action: &str,
    value: &str,
    state: &mut AppState,
    runtime: &AppRuntime,
) {
    match action {
        "nsec" => {
            let trimmed = value.trim();
            let result = if trimmed.starts_with("bunker://") {
                runtime.sign_in_bunker(trimmed)
            } else {
                runtime.sign_in_nsec(trimmed)
            };
            match result {
                Ok(()) => state.push_toast("\u{2713} signing in…"),
                Err(e) => state.push_toast(&format!("\u{2717} sign-in failed: {e}")),
            }
        }
        "nwc" => match runtime.wallet_connect(value.trim()) {
            Ok(()) => state.push_toast("\u{2713} wallet connect requested"),
            Err(e) => state.push_toast(&format!("\u{2717} wallet connect failed: {e}")),
        },
        "bolt11" => match runtime.wallet_pay_invoice(value.trim(), None) {
            Ok(()) => state.push_toast("\u{2713} payment requested"),
            Err(e) => state.push_toast(&format!("\u{2717} pay failed: {e}")),
        },
        "relay" => match runtime.add_relay(value, "both,indexer") {
            Ok(()) => state.push_toast("\u{2713} relay add requested"),
            Err(e) => state.push_toast(&format!("\u{2717} add relay failed: {e}")),
        },
        "zap-amount" => dispatch_zap_amount(value, state, runtime),
        _ => {
            state.push_toast(&format!("unknown action: {action}"));
        }
    }
}

fn dispatch_zap_amount(value: &str, state: &mut AppState, runtime: &AppRuntime) {
    let pubkey = match state.pending_zap_pubkey.take() {
        Some(p) => p,
        None => {
            state.push_toast("\u{2717} zap context lost");
            return;
        }
    };
    let event_id = state.pending_zap_event_id.take();
    let trimmed = value.trim();
    let (sats_str, comment) = trimmed
        .split_once(char::is_whitespace)
        .map(|(s, c)| (s, c.trim()))
        .unwrap_or((trimmed, ""));
    let sats: u64 = match sats_str.parse() {
        Ok(n) if n > 0 => n,
        _ => {
            state.push_toast("\u{2717} enter a positive number of sats");
            return;
        }
    };
    match runtime.zap(
        &pubkey,
        sats * 1000,
        event_id.as_deref(),
        (!comment.is_empty()).then_some(comment),
        None,
    ) {
        Ok(cid) => {
            state.track_action(cid, &format!("zap {sats} sat"));
        }
        Err(e) => state.push_toast(&format!("\u{2717} zap failed: {e}")),
    }
}

pub(super) fn handle_modal_key(state: &mut AppState, runtime: &AppRuntime, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => state.cancel_modal(),
        KeyCode::Tab => state.next_modal_field(),
        KeyCode::BackTab => state.prev_modal_field(),
        KeyCode::Backspace => state.backspace_modal(),
        KeyCode::Enter => {
            if let Some((action, fields)) = state.take_modal() {
                dispatch_modal_action(&action, &fields, state, runtime);
            }
        }
        KeyCode::Char(ch) if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT => {
            state.push_modal_char(ch);
        }
        _ => {}
    }
}

fn dispatch_modal_action(
    action: &str,
    fields: &[(String, String)],
    state: &mut AppState,
    runtime: &AppRuntime,
) {
    match action {
        "create-account" => {
            let name = fields.first().map(|(_, v)| v.trim()).unwrap_or("anon");
            match runtime.create_account(name, &[], false) {
                Ok(()) => state.push_toast("\u{2713} account creation requested…"),
                Err(e) => state.push_toast(&format!("\u{2717} create failed: {e}")),
            }
        }
        "bunker-connect" => {
            let uri = fields.first().map(|(_, v)| v.trim()).unwrap_or("");
            match runtime.sign_in_bunker(uri) {
                Ok(()) => state.push_toast("\u{2713} bunker connect requested…"),
                Err(e) => state.push_toast(&format!("\u{2717} bunker failed: {e}")),
            }
        }
        super::group_forms::CREATE_GROUP_ACTION => {
            super::group_forms::dispatch_create_group(fields, state, runtime);
        }
        _ => state.push_toast(&format!("\u{2717} unknown modal action '{action}'")),
    }
}

pub(super) fn handle_note_details_modal_key(state: &mut AppState, key: KeyEvent) {
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => state.close_note_details_modal(),
        KeyCode::Char('j') | KeyCode::Down => state.scroll_note_details_modal_down(),
        KeyCode::Char('k') | KeyCode::Up => state.scroll_note_details_modal_up(),
        _ => {}
    }
}

pub(super) fn handle_account_switcher_key(
    state: &mut AppState,
    runtime: &AppRuntime,
    key: KeyEvent,
) {
    let n = state.features.accounts.len();
    match key.code {
        KeyCode::Esc => state.close_account_switcher(),
        KeyCode::Char('j') | KeyCode::Down => {
            if n > 0 {
                state.account_switcher_cursor = (state.account_switcher_cursor + 1) % n;
            }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            if n > 0 {
                state.account_switcher_cursor = state.account_switcher_cursor.saturating_sub(1);
            }
        }
        KeyCode::Enter => {
            if let Some(account) = state.features.accounts.get(state.account_switcher_cursor) {
                let id = account.id.clone();
                let name = account.display.clone();
                state.close_account_switcher();
                match runtime.switch_account(&id) {
                    Ok(()) => state.push_toast(&format!("\u{2713} switched to @{name}")),
                    Err(e) => state.push_toast(&format!("\u{2717} switch failed: {e}")),
                }
            } else {
                state.close_account_switcher();
            }
        }
        _ => {}
    }
}
