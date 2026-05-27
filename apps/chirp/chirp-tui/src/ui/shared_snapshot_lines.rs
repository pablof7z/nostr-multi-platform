use crate::app::AppState;
use crate::short_id;

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
