use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::{AppRuntime, AppState, Mode, Pane};
use crate::features::FeatureTab;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputFlow {
    Continue,
    Quit,
}

pub fn handle_key(state: &mut AppState, runtime: &AppRuntime, key: KeyEvent) -> InputFlow {
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return InputFlow::Quit;
    }

    match state.mode {
        Mode::Compose => {
            // q always quits; ? always toggles help even while composing.
            if key.code == KeyCode::Char('q') {
                return InputFlow::Quit;
            }
            if key.code == KeyCode::Char('?') {
                state.toggle_help();
                return InputFlow::Continue;
            }
            handle_compose_key(state, runtime, key);
            return InputFlow::Continue;
        }
        Mode::Command => {
            handle_command_key(state, runtime, key);
            return InputFlow::Continue;
        }
        Mode::Palette { .. } => {
            handle_palette_key(state, runtime, key);
            return InputFlow::Continue;
        }
        Mode::InputBar => {
            handle_input_bar_key(state, runtime, key);
            return InputFlow::Continue;
        }
        Mode::ModalForm => {
            handle_modal_key(state, runtime, key);
            return InputFlow::Continue;
        }
        Mode::AccountSwitcher => {
            handle_account_switcher_key(state, runtime, key);
            return InputFlow::Continue;
        }
        Mode::Normal => {}
    }

    match key.code {
        KeyCode::Char('q') => return InputFlow::Quit,
        KeyCode::Char('/') if state.mode == Mode::Normal => state.open_palette(),
        KeyCode::Char('?') => state.toggle_help(),
        KeyCode::Char(':') => {
            state.push_toast("Commands removed — press ? for help or / for palette");
        }
        KeyCode::Char('a') => state.open_account_switcher(),
        KeyCode::Char('c') if state.features.accounts.is_empty() => {
            state.start_modal("Create account", vec!["Display name"], "create-account");
        }
        KeyCode::Tab => state.next_tab(),
        KeyCode::BackTab => state.previous_tab(),
        KeyCode::Char('l') | KeyCode::Right
            if state.mode == Mode::Normal && state.focused != Pane::Detail =>
        {
            state.focused = Pane::Detail;
            state.detail_cursor = 0;
            state.detail_scroll = 0;
            state.status = "focus:detail".to_string();
        }
        KeyCode::Char('h') | KeyCode::Left
            if state.mode == Mode::Normal && state.focused == Pane::Detail =>
        {
            state.focused = Pane::Feed;
            state.status = "focus:feed".to_string();
        }
        KeyCode::Char('j') | KeyCode::Down
            if state.mode == Mode::Normal && state.focused == Pane::Detail =>
        {
            let reply_count = count_replies_for_selected(state);
            state.detail_cursor = (state.detail_cursor + 1).min(reply_count);
        }
        KeyCode::Char('k') | KeyCode::Up
            if state.mode == Mode::Normal && state.focused == Pane::Detail =>
        {
            state.detail_cursor = state.detail_cursor.saturating_sub(1);
        }
        KeyCode::Char('J') if state.mode == Mode::Normal && state.focused == Pane::Detail => {
            state.detail_scroll = state.detail_scroll.saturating_add(1);
        }
        KeyCode::Char('K') if state.mode == Mode::Normal && state.focused == Pane::Detail => {
            state.detail_scroll = state.detail_scroll.saturating_sub(1);
        }
        KeyCode::Char(ch) if FeatureTab::from_key(ch).is_some() => {
            if let Some(tab) = FeatureTab::from_key(ch) {
                state.set_tab(tab);
            }
        }
        KeyCode::Char('1') => state.focus(Pane::Feed),
        KeyCode::Char('2') => state.focus(Pane::Detail),
        KeyCode::Char('3') => state.focus(Pane::Profile),
        KeyCode::Down | KeyCode::Char('j') if state.focused != Pane::Detail => match state.tab {
            crate::features::FeatureTab::Chats => state.chat_select_next(),
            crate::features::FeatureTab::Groups => state.group_select_next(),
            crate::features::FeatureTab::Settings => state.settings_account_select_next(),
            _ => {
                state.select_next();
                state.load_older_timeline_if_needed(runtime);
            }
        },
        KeyCode::Up | KeyCode::Char('k') if state.focused != Pane::Detail => match state.tab {
            crate::features::FeatureTab::Chats => state.chat_select_previous(),
            crate::features::FeatureTab::Groups => state.group_select_previous(),
            crate::features::FeatureTab::Settings => state.settings_account_select_previous(),
            _ => state.select_previous(),
        },
        KeyCode::PageDown => {
            state.select_page_down();
            if state.tab == FeatureTab::Home {
                state.load_older_timeline_if_needed(runtime);
            }
        }
        KeyCode::PageUp => state.select_page_up(),
        KeyCode::Home => state.select_first(),
        KeyCode::End => {
            state.select_last();
            if state.tab == FeatureTab::Home {
                state.load_older_timeline(runtime);
            }
        }
        KeyCode::Enter => open_selected_thread(state, runtime),
        KeyCode::Char('p') => {
            if state.tab == FeatureTab::Wallet {
                state.start_input_bar("bolt11 invoice", false, "bolt11");
            } else {
                open_selected_author(state, runtime);
            }
        }
        KeyCode::Char('n') => handle_n_key(state, runtime),
        KeyCode::Char('i') => match state.tab {
            FeatureTab::Chats => {
                state.chat_composing = !state.chat_composing;
            }
            FeatureTab::Groups => {
                state.group_composing = !state.group_composing;
            }
            _ => {}
        },
        KeyCode::Char('r') => state.start_reply(),
        KeyCode::Char('z') => handle_z_key(state, runtime),
        KeyCode::Char('+') => react_to_selected(state, runtime),
        KeyCode::Char('f') => follow_selected(state, runtime, true),
        KeyCode::Char('F') => follow_selected(state, runtime, false),
        KeyCode::Esc => {
            if !state.close_help() {
                state.status = "detail closed".to_string();
            }
        }
        _ => {}
    }
    InputFlow::Continue
}

fn handle_compose_key(state: &mut AppState, runtime: &AppRuntime, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => state.cancel_compose(),
        KeyCode::Backspace => state.backspace_compose(),
        KeyCode::Enter if key.modifiers.contains(KeyModifiers::SHIFT) => {
            state.push_compose_newline()
        }
        KeyCode::Enter => publish_compose(state, runtime),
        KeyCode::Char(ch) if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT => {
            state.push_compose_char(ch)
        }
        _ => {}
    }
}

fn handle_command_key(state: &mut AppState, runtime: &AppRuntime, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => state.cancel_command(),
        KeyCode::Backspace => state.backspace_command(),
        KeyCode::Enter => {
            if let Some(command) = state.take_command() {
                crate::commands::execute(&command, state, runtime);
            }
        }
        KeyCode::Char(ch) if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT => {
            state.push_command_char(ch)
        }
        _ => {}
    }
}

fn publish_compose(state: &mut AppState, runtime: &AppRuntime) {
    let Some((content, reply_to)) = state.take_compose() else {
        return;
    };
    match runtime.publish_note(&content, reply_to.as_deref()) {
        Ok(correlation_id) => state.track_action(correlation_id, "note publish"),
        Err(error) => state.status = format!("publish failed: {error}"),
    }
}

fn open_selected_thread(state: &mut AppState, runtime: &AppRuntime) {
    let Some(row) = state.selected_row().cloned() else {
        state.status = "select a note before opening a thread".to_string();
        return;
    };
    match runtime.open_thread(&row.id) {
        Ok(()) => {
            state.focus(Pane::Detail);
            state.status = format!("opened thread {}", short(&row.id));
        }
        Err(error) => state.status = format!("open thread failed: {error}"),
    }
}

fn open_selected_author(state: &mut AppState, runtime: &AppRuntime) {
    let Some(row) = state.selected_row().cloned() else {
        state.status = "select a note before opening a profile".to_string();
        return;
    };
    state.profile_pubkey = row.author_pubkey.clone();
    match runtime.open_author(&row.author_pubkey) {
        Ok(()) => {
            state.focus(Pane::Profile);
            state.status = format!("opened profile {}", row.author_label());
        }
        Err(error) => state.status = format!("open profile failed: {error}"),
    }
}

fn react_to_selected(state: &mut AppState, runtime: &AppRuntime) {
    let Some(row) = state.selected_row().cloned() else {
        state.status = "select a note before reacting".to_string();
        return;
    };
    match runtime.react(&row.id, "+") {
        Ok(correlation_id) => state.track_action(
            correlation_id,
            &format!("+ reaction for {}", short(&row.id)),
        ),
        Err(error) => state.status = format!("reaction failed: {error}"),
    }
}

fn follow_selected(state: &mut AppState, runtime: &AppRuntime, add: bool) {
    let Some(row) = state.selected_row().cloned() else {
        state.status = "select a note before changing follows".to_string();
        return;
    };
    match runtime.follow(&row.author_pubkey, add) {
        Ok(correlation_id) if add => {
            state.track_action(correlation_id, &format!("follow {}", row.author_label()))
        }
        Ok(correlation_id) => {
            state.track_action(correlation_id, &format!("unfollow {}", row.author_label()))
        }
        Err(error) => state.status = format!("follow action failed: {error}"),
    }
}

fn handle_palette_key(state: &mut AppState, runtime: &AppRuntime, key: KeyEvent) {
    let actions = crate::ui::palette::actions_for_state(state);
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => state.close_palette(),
        KeyCode::Char('j') | KeyCode::Down => {
            if let Mode::Palette { ref mut cursor } = state.mode {
                *cursor = (*cursor + 1).min(actions.len().saturating_sub(1));
            }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            if let Mode::Palette { ref mut cursor } = state.mode {
                *cursor = cursor.saturating_sub(1);
            }
        }
        KeyCode::Enter => {
            let cursor = if let Mode::Palette { cursor } = state.mode {
                cursor
            } else {
                0
            };
            if let Some(&action) = actions.get(cursor) {
                dispatch_palette_action(action, state, runtime);
            }
            state.close_palette();
        }
        _ => {}
    }
}

fn dispatch_palette_action(action: &str, state: &mut AppState, runtime: &AppRuntime) {
    let (note_id, author_pubkey) = if state.focused == Pane::Detail && state.detail_cursor > 0 {
        let reply_idx = state.selected.saturating_add(state.detail_cursor);
        if let Some(row) = state.rows.get(reply_idx) {
            (row.id.clone(), row.author_pubkey.clone())
        } else {
            return;
        }
    } else if let Some(row) = state.selected_row().cloned() {
        (row.id.clone(), row.author_pubkey.clone())
    } else {
        return;
    };

    match action {
        "View profile" => {
            state.profile_pubkey = author_pubkey.clone();
            if runtime.open_author(&author_pubkey).is_ok() {
                state.focus(Pane::Profile);
                state.status = "opened profile".to_string();
            }
        }
        "React \u{2665}" => match runtime.react(&note_id, "+") {
            Ok(cid) => state.track_action(cid, "reaction"),
            Err(e) => state.status = format!("react failed: {e}"),
        },
        "Follow" => match runtime.follow(&author_pubkey, true) {
            Ok(cid) => state.track_action(cid, "follow"),
            Err(e) => state.status = format!("follow failed: {e}"),
        },
        "Unfollow" => match runtime.follow(&author_pubkey, false) {
            Ok(cid) => state.track_action(cid, "unfollow"),
            Err(e) => state.status = format!("unfollow failed: {e}"),
        },
        "Repost" => state.status = "repost not yet wired (post-v1)".to_string(),
        "Reply" => state.start_reply(),
        "Zap" => {
            state.pending_zap_pubkey = Some(author_pubkey);
            state.pending_zap_event_id = Some(note_id);
            state.start_input_bar("sats [comment]", false, "zap-amount");
        }
        _ => {}
    }
}

fn short(value: &str) -> String {
    if value.len() <= 14 {
        value.to_string()
    } else {
        format!("{}...{}", &value[..8], &value[value.len() - 4..])
    }
}

fn handle_z_key(state: &mut AppState, _runtime: &AppRuntime) {
    if let Some(row) = state.selected_row().cloned() {
        state.pending_zap_pubkey = Some(row.author_pubkey);
        state.pending_zap_event_id = Some(row.id);
        state.start_input_bar("sats [comment]", false, "zap-amount");
    }
}

fn count_replies_for_selected(state: &AppState) -> usize {
    let start = state.selected.saturating_add(1);
    if start >= state.rows.len() {
        return 0;
    }
    state.rows[start..]
        .iter()
        .take_while(|r| r.depth > 0)
        .count()
}

fn handle_n_key(state: &mut AppState, _runtime: &AppRuntime) {
    if state.features.accounts.is_empty() {
        state.start_input_bar("nsec  (or bunker:// URI)", false, "nsec");
        return;
    }
    match state.tab {
        FeatureTab::Home => state.start_compose(),
        FeatureTab::Chats => state.start_input_bar("New DM to", false, "dm-npub"),
        FeatureTab::Groups => state.push_toast("\u{2717} group discover not yet wired"),
        FeatureTab::Wallet => state.start_input_bar("NWC URI", false, "nwc"),
        FeatureTab::Settings => state.push_toast("\u{2717} add relay/account not yet wired"),
    }
}

fn handle_input_bar_key(state: &mut AppState, runtime: &AppRuntime, key: KeyEvent) {
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
        "zap-amount" => {
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
            let mut body = serde_json::json!({
                "recipient_pubkey": pubkey,
                "amount_msats": sats * 1000,
            });
            if let Some(id) = event_id {
                body["target_event_id"] = serde_json::Value::String(id);
            }
            if !comment.is_empty() {
                body["comment"] = serde_json::Value::String(comment.to_string());
            }
            match runtime.zap(&body) {
                Ok(cid) => {
                    state.pending_zap_pay = true;
                    state.track_action(cid, &format!("zap {sats} sat"));
                }
                Err(e) => state.push_toast(&format!("\u{2717} zap failed: {e}")),
            }
        }
        "dm-npub" => {
            state.push_toast("\u{2717} DM open not yet wired");
        }
        _ => {
            state.push_toast(&format!("unknown action: {action}"));
        }
    }
}

fn handle_modal_key(state: &mut AppState, runtime: &AppRuntime, key: KeyEvent) {
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
        _ => state.push_toast(&format!("\u{2717} modal action '{action}' not wired")),
    }
}

fn handle_account_switcher_key(state: &mut AppState, runtime: &AppRuntime, key: KeyEvent) {
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
