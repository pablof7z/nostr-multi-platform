use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::{AppRuntime, AppState, Mode, Pane};
use crate::features::FeatureTab;

mod feed_navigation;
mod forms;
mod group_forms;
mod outbox;

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
            forms::handle_input_bar_key(state, runtime, key);
            return InputFlow::Continue;
        }
        Mode::ModalForm => {
            forms::handle_modal_key(state, runtime, key);
            return InputFlow::Continue;
        }
        Mode::AccountSwitcher => {
            forms::handle_account_switcher_key(state, runtime, key);
            return InputFlow::Continue;
        }
        Mode::NoteDetailsModal { .. } => {
            forms::handle_note_details_modal_key(state, key);
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
        KeyCode::Tab => feed_navigation::set_tab_closing_dynamic(state, runtime, state.tab.next()),
        KeyCode::BackTab => {
            feed_navigation::set_tab_closing_dynamic(state, runtime, state.tab.previous());
        }
        KeyCode::Char('l') | KeyCode::Right
            if state.mode == Mode::Normal && state.tab == FeatureTab::Settings =>
        {
            state.settings_section_next();
        }
        KeyCode::Char('h') | KeyCode::Left
            if state.mode == Mode::Normal && state.tab == FeatureTab::Settings =>
        {
            state.settings_section_previous();
        }
        KeyCode::Char('l') | KeyCode::Right
            if state.mode == Mode::Normal && state.focused != Pane::Detail =>
        {
            if state.focused == Pane::Profile
                && !feed_navigation::close_author_before_detail_focus(state, runtime)
            {
                return InputFlow::Continue;
            }
            state.focused = Pane::Detail;
            state.detail_cursor = 0;
            state.detail_scroll = 0;
            state.status = "focus:detail".to_string();
        }
        KeyCode::Char('h') | KeyCode::Left
            if state.mode == Mode::Normal && state.focused == Pane::Detail =>
        {
            feed_navigation::close_current_dynamic_view(state, runtime);
        }
        KeyCode::Char('h') | KeyCode::Left
            if state.mode == Mode::Normal && state.focused == Pane::Profile =>
        {
            feed_navigation::close_current_dynamic_view(state, runtime);
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
                feed_navigation::set_tab_closing_dynamic(state, runtime, tab);
            }
        }
        KeyCode::Char('1') => feed_navigation::close_current_dynamic_view(state, runtime),
        KeyCode::Char('2') => {
            if feed_navigation::close_author_before_detail_focus(state, runtime) {
                state.focus(Pane::Detail);
            }
        }
        KeyCode::Char('3') => feed_navigation::close_thread_before_profile_focus(state, runtime),
        KeyCode::Down | KeyCode::Char('j') if state.focused != Pane::Detail => match state.tab {
            crate::features::FeatureTab::Chats => state.chat_select_next(),
            crate::features::FeatureTab::Groups => state.group_select_next(),
            crate::features::FeatureTab::Settings => match state.settings_cursor {
                0 => state.settings_account_select_next(),
                1 => state.settings_relay_select_next(),
                2 => outbox::select_next(state),
                _ => {}
            },
            _ => {
                state.select_next();
                state.load_older_timeline_if_needed(runtime);
            }
        },
        KeyCode::Up | KeyCode::Char('k') if state.focused != Pane::Detail => match state.tab {
            crate::features::FeatureTab::Chats => state.chat_select_previous(),
            crate::features::FeatureTab::Groups => state.group_select_previous(),
            crate::features::FeatureTab::Settings => match state.settings_cursor {
                0 => state.settings_account_select_previous(),
                1 => state.settings_relay_select_previous(),
                2 => outbox::select_previous(state),
                _ => {}
            },
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
        KeyCode::Enter => match state.tab {
            FeatureTab::Home => open_selected_thread(state, runtime),
            FeatureTab::Settings => outbox::open_or_focus(state),
            _ => {}
        },
        KeyCode::Char('r') if state.tab == FeatureTab::Settings && outbox::is_open(state) => {
            outbox::retry_selected(state, runtime);
        }
        KeyCode::Char('d') if state.tab == FeatureTab::Settings && outbox::is_open(state) => {
            outbox::clear_or_cancel_selected(state, runtime);
        }
        KeyCode::Char('p') => {
            if state.tab == FeatureTab::Wallet {
                state.start_input_bar("bolt11 invoice", false, "bolt11");
            } else {
                open_selected_author(state, runtime);
            }
        }
        KeyCode::Char('n') => handle_n_key(state, runtime),
        KeyCode::Char('i') => match state.tab {
            FeatureTab::Home => {
                if let Some(row) = state.selected_row() {
                    state.open_note_details_modal(row.note_details_text());
                }
            }
            _ => {}
        },
        KeyCode::Char('r') => state.start_reply(),
        KeyCode::Char('R') if state.tab == FeatureTab::Home => repost_selected(state, runtime),
        KeyCode::Char('z') => handle_z_key(state, runtime),
        KeyCode::Char('+') => react_to_selected(state, runtime),
        KeyCode::Char('f') => follow_selected(state, runtime, true),
        KeyCode::Char('F') => follow_selected(state, runtime, false),
        KeyCode::Esc => {
            feed_navigation::handle_escape(state, runtime);
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
    match runtime.publish_note(&content, reply_to.as_ref()) {
        Ok(correlation_id) => state.track_action(correlation_id, "note publish"),
        Err(error) => state.status = format!("publish failed: {error}"),
    }
}

fn open_selected_thread(state: &mut AppState, runtime: &AppRuntime) {
    let Some(row) = state.selected_row().cloned() else {
        state.status = "select a note before opening a thread".to_string();
        return;
    };
    match state.open_thread_feed(runtime, &row.id) {
        Ok(()) => {
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
    match state.open_author_feed(runtime, &row.author_pubkey) {
        Ok(()) => {
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

fn repost_selected(state: &mut AppState, runtime: &AppRuntime) {
    let Some(row) = state.selected_row().cloned() else {
        state.status = "select a note before reposting".to_string();
        return;
    };
    repost_note(state, runtime, &row.id, &row.author_pubkey);
}

fn repost_note(state: &mut AppState, runtime: &AppRuntime, note_id: &str, author_pubkey: &str) {
    match runtime.repost(note_id, author_pubkey) {
        Ok(correlation_id) => {
            state.track_action(correlation_id, &format!("repost {}", short(note_id)))
        }
        Err(error) => state.status = format!("repost failed: {error}"),
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
    let row = if state.focused == Pane::Detail && !state.thread_event_id.is_empty() {
        let Some(row) = state.thread_rows.get(state.detail_cursor).cloned() else {
            return;
        };
        row
    } else if state.focused == Pane::Detail && state.detail_cursor > 0 {
        let reply_idx = state.selected.saturating_add(state.detail_cursor);
        if let Some(row) = state.rows.get(reply_idx) {
            row.clone()
        } else {
            return;
        }
    } else if let Some(row) = state.selected_row().cloned() {
        row
    } else {
        return;
    };
    let note_id = row.id.clone();
    let author_pubkey = row.author_pubkey.clone();

    match action {
        "View profile" => match state.open_author_feed(runtime, &author_pubkey) {
            Ok(()) => state.status = "opened profile".to_string(),
            Err(error) => state.status = format!("open profile failed: {error}"),
        },
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
        "Repost" => repost_note(state, runtime, &note_id, &author_pubkey),
        "Reply" => state.start_reply(),
        "View note details" => state.open_note_details_modal(row.note_details_text()),
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
    if !state.thread_event_id.is_empty() {
        return state.thread_rows.len().saturating_sub(1);
    }
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
        FeatureTab::Chats => {}
        FeatureTab::Groups => group_forms::start_create_group(state),
        FeatureTab::Wallet => state.start_input_bar("NWC URI", false, "nwc"),
        FeatureTab::Settings => handle_settings_n_key(state),
    }
}

fn handle_settings_n_key(state: &mut AppState) {
    match state.settings_cursor {
        0 => state.start_modal("Create account", vec!["Display name"], "create-account"),
        1 => state.start_input_bar("Relay URL", false, "relay"),
        2 => state.status = "outbox has no create action; press Enter for details".to_string(),
        _ => {}
    }
}

#[cfg(test)]
mod tests;
