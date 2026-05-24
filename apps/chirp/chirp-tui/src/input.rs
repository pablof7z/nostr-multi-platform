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

    if state.mode == Mode::Compose {
        handle_compose_key(state, runtime, key);
        return InputFlow::Continue;
    }
    if state.mode == Mode::Command {
        handle_command_key(state, runtime, key);
        return InputFlow::Continue;
    }

    match key.code {
        KeyCode::Char('q') => return InputFlow::Quit,
        KeyCode::Char('?') => state.toggle_help(),
        KeyCode::Char(':') => state.start_command(),
        KeyCode::Tab => state.next_tab(),
        KeyCode::BackTab => state.previous_tab(),
        KeyCode::Char(ch) if FeatureTab::from_key(ch).is_some() => {
            if let Some(tab) = FeatureTab::from_key(ch) {
                state.set_tab(tab);
            }
        }
        KeyCode::Char('1') => state.focus(Pane::Feed),
        KeyCode::Char('2') => state.focus(Pane::Detail),
        KeyCode::Char('3') => state.focus(Pane::Profile),
        KeyCode::Down | KeyCode::Char('j') => state.select_next(),
        KeyCode::Up | KeyCode::Char('k') => state.select_previous(),
        KeyCode::PageDown => state.select_page_down(),
        KeyCode::PageUp => state.select_page_up(),
        KeyCode::Home => state.select_first(),
        KeyCode::End => state.select_last(),
        KeyCode::Enter => open_selected_thread(state, runtime),
        KeyCode::Char('p') => open_selected_author(state, runtime),
        KeyCode::Char('i') => state.start_compose(),
        KeyCode::Char('r') => state.start_reply(),
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
        KeyCode::Enter if key.modifiers.contains(KeyModifiers::CONTROL) => {
            publish_compose(state, runtime)
        }
        KeyCode::Enter => state.push_compose_newline(),
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
    match runtime.open_author(&row.author_pubkey) {
        Ok(()) => {
            state.focus(Pane::Profile);
            state.status = format!("opened profile {}", row.author);
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
            state.track_action(correlation_id, &format!("follow {}", row.author))
        }
        Ok(correlation_id) => {
            state.track_action(correlation_id, &format!("unfollow {}", row.author))
        }
        Err(error) => state.status = format!("follow action failed: {error}"),
    }
}

fn short(value: &str) -> String {
    if value.len() <= 14 {
        value.to_string()
    } else {
        format!("{}...{}", &value[..8], &value[value.len() - 4..])
    }
}
