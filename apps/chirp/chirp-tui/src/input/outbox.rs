use crate::app::{AppRuntime, AppState, OutboxSelection};
use crate::short_id;

pub fn is_open(state: &AppState) -> bool {
    state.outbox_selected.is_some()
}

pub fn select_next(state: &mut AppState) {
    move_selection(state, 1);
}

pub fn select_previous(state: &mut AppState) {
    move_selection(state, -1);
}

pub fn open_or_focus(state: &mut AppState) {
    if total_rows(state) == 0 {
        state.outbox_selected = None;
        state.status = "no publish rows to open".to_string();
        return;
    }
    if state.outbox_selected.is_none() {
        state.outbox_selected = Some(first_selection(state));
    }
    state.clamp_outbox_selection();
    state.status = "outbox detail: r retry, d clear/cancel, Esc close".to_string();
}

pub fn close(state: &mut AppState) -> bool {
    if state.outbox_selected.is_some() {
        state.outbox_selected = None;
        state.status = "outbox detail closed".to_string();
        return true;
    }
    false
}

pub fn retry_selected(state: &mut AppState, runtime: &AppRuntime) {
    let Some((handle, can_retry)) = selected_handle(state) else {
        open_or_focus(state);
        return;
    };
    if !can_retry {
        state.status = "selected publish is not retryable".to_string();
        return;
    }
    match runtime.retry_publish(&handle) {
        Ok(()) => state.status = format!("retry requested for {}", short_id(&handle)),
        Err(error) => state.status = format!("retry failed: {error}"),
    }
}

pub fn clear_or_cancel_selected(state: &mut AppState, runtime: &AppRuntime) {
    let Some((handle, _)) = selected_handle(state) else {
        open_or_focus(state);
        return;
    };
    let action = match state.outbox_selected {
        Some(OutboxSelection::Active(_)) => "cancel",
        Some(OutboxSelection::History(_)) => "clear",
        None => "cancel",
    };
    match runtime.cancel_publish(&handle) {
        Ok(()) => {
            state.status = format!("{action} requested for {}", short_id(&handle));
            if action == "clear" {
                state.outbox_selected = None;
            }
        }
        Err(error) => state.status = format!("{action} failed: {error}"),
    }
}

fn move_selection(state: &mut AppState, delta: isize) {
    let total = total_rows(state);
    if total == 0 {
        state.outbox_selected = None;
        return;
    }
    let current = state
        .outbox_selected
        .map(|selection| selection_to_linear(state, selection))
        .unwrap_or(0);
    let next = if delta.is_negative() {
        current.saturating_sub(delta.unsigned_abs())
    } else {
        current
            .saturating_add(delta as usize)
            .min(total.saturating_sub(1))
    };
    state.outbox_selected = Some(linear_to_selection(state, next));
}

fn selected_handle(state: &AppState) -> Option<(String, bool)> {
    match state.outbox_selected? {
        OutboxSelection::Active(i) => state
            .features
            .outbox
            .get(i)
            .map(|row| (row.handle.clone(), row.can_retry)),
        OutboxSelection::History(i) => state
            .features
            .history
            .get(i)
            .map(|row| (row.event_id.clone(), row.can_retry)),
    }
}

fn first_selection(state: &AppState) -> OutboxSelection {
    if state.features.outbox.is_empty() {
        OutboxSelection::History(0)
    } else {
        OutboxSelection::Active(0)
    }
}

fn total_rows(state: &AppState) -> usize {
    state.features.outbox.len() + state.features.history.len()
}

fn selection_to_linear(state: &AppState, selection: OutboxSelection) -> usize {
    match selection {
        OutboxSelection::Active(i) => i.min(state.features.outbox.len().saturating_sub(1)),
        OutboxSelection::History(i) => state.features.outbox.len() + i,
    }
}

fn linear_to_selection(state: &AppState, idx: usize) -> OutboxSelection {
    let active_len = state.features.outbox.len();
    if idx < active_len {
        OutboxSelection::Active(idx)
    } else {
        OutboxSelection::History(idx.saturating_sub(active_len))
    }
}
