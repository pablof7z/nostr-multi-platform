use ratatui::text::Line;

use crate::app::AppState;

pub fn action_summary(state: &AppState) -> String {
    if let Some(result) = &state.last_action_result {
        return match result.error.as_deref() {
            Some(error) if !error.is_empty() => {
                format!("last action: {} {}", result.status, error)
            }
            _ => format!("last action: {}", result.status),
        };
    }
    if let Some(stage) = state.action_stages.first() {
        return match stage.reason.as_deref() {
            Some(reason) if !reason.is_empty() => {
                format!(
                    "action {}: {} {}",
                    short_id(&stage.correlation_id),
                    stage.stage,
                    reason
                )
            }
            _ => format!(
                "action {}: {}",
                short_id(&stage.correlation_id),
                stage.stage
            ),
        };
    }
    "last action: none".to_string()
}

pub fn relay_lines(state: &AppState) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from(format!(
        "Relays: {}  Interests: {}",
        state.relays.len(),
        state.interests.len()
    ))];
    if state.relays.is_empty() {
        lines.push(Line::from("No relay diagnostics yet."));
        return lines;
    }
    for relay in state.relays.iter().take(4) {
        let last = relay.last_event_display.as_deref().unwrap_or("no events");
        lines.push(Line::from(format!(
            "{}  {}  {}  subs:{}  events:{}  {}",
            relay.short_url,
            relay.role_label,
            relay.connection_label,
            relay.active_sub_count,
            relay.total_events_display,
            last
        )));
        if let Some(error) = &relay.last_error {
            lines.push(Line::from(format!("  error: {error}")));
        }
    }
    lines
}

fn short_id(value: &str) -> String {
    if value.len() <= 16 {
        value.to_string()
    } else {
        format!("{}...{}", &value[..8], &value[value.len() - 6..])
    }
}
