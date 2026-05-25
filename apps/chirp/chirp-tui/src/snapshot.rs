use std::collections::BTreeMap;

use serde_json::Value;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SharedSnapshot {
    pub metrics: RuntimeMetrics,
    pub relays: Vec<RelayRow>,
    pub interests: Vec<InterestRow>,
    pub action_results: Vec<ActionResult>,
    pub action_stages: Vec<ActionStageRow>,
    pub mention_profiles: BTreeMap<String, MentionProfile>,
}

impl SharedSnapshot {
    #[must_use]
    pub fn from_payload(payload: &str) -> Self {
        let Ok(value) = serde_json::from_str::<Value>(payload) else {
            return Self::default();
        };
        Self::from_value(&value)
    }

    fn from_value(value: &Value) -> Self {
        let projections = value.get("projections");
        Self {
            metrics: RuntimeMetrics::from_value(value.get("metrics")),
            relays: relays_from(projections),
            interests: interests_from(projections),
            action_results: action_results_from(projections),
            action_stages: action_stages_from(projections),
            mention_profiles: mention_profiles_from(projections),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RuntimeMetrics {
    pub events_rx: u64,
    pub visible_items: u64,
    pub actor_queue_depth: u64,
    pub update_sequence: u64,
}

impl RuntimeMetrics {
    fn from_value(metrics: Option<&Value>) -> Self {
        let Some(metrics) = metrics else {
            return Self::default();
        };
        Self {
            events_rx: number_field(metrics, "events_rx"),
            visible_items: number_field(metrics, "visible_items"),
            actor_queue_depth: number_field(metrics, "actor_queue_depth"),
            update_sequence: number_field(metrics, "update_sequence"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelayRow {
    pub short_url: String,
    pub role_label: String,
    pub connection_label: String,
    pub active_sub_count: u64,
    pub total_events_display: String,
    pub last_event_display: Option<String>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterestRow {
    pub key: String,
    pub state: String,
    pub refcount: u64,
    pub cache_coverage: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionResult {
    pub correlation_id: String,
    pub status: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionStageRow {
    pub correlation_id: String,
    pub stage: String,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MentionProfile {
    pub display: String,
    pub picture_url: String,
    pub avatar_initials: String,
    pub avatar_color: String,
}

fn relays_from(projections: Option<&Value>) -> Vec<RelayRow> {
    projections
        .and_then(|p| p.get("relay_diagnostics"))
        .and_then(|diag| diag.get("relays"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|row| RelayRow {
            short_url: string_field(row, "short_url"),
            role_label: string_field(row, "role_label"),
            connection_label: string_field(row, "connection_label"),
            active_sub_count: number_field(row, "active_sub_count"),
            total_events_display: string_field(row, "total_events_display"),
            last_event_display: optional_string(row, "last_event_display"),
            last_error: optional_string(row, "last_error"),
        })
        .collect()
}

fn interests_from(projections: Option<&Value>) -> Vec<InterestRow> {
    projections
        .and_then(|p| p.get("relay_diagnostics"))
        .and_then(|diag| diag.get("interests"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|row| InterestRow {
            key: string_field(row, "key"),
            state: string_field(row, "state"),
            refcount: number_field(row, "refcount"),
            cache_coverage: string_field(row, "cache_coverage"),
        })
        .collect()
}

fn action_results_from(projections: Option<&Value>) -> Vec<ActionResult> {
    projections
        .and_then(|p| p.get("action_results"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|row| ActionResult {
            correlation_id: string_field(row, "correlation_id"),
            status: string_field(row, "status"),
            error: optional_string(row, "error"),
        })
        .filter(|row| !row.correlation_id.is_empty())
        .collect()
}

fn action_stages_from(projections: Option<&Value>) -> Vec<ActionStageRow> {
    let mut rows = Vec::new();
    let Some(stages) = projections
        .and_then(|p| p.get("action_stages"))
        .and_then(Value::as_object)
    else {
        return rows;
    };

    for (correlation_id, entries) in stages {
        let Some(last) = entries.as_array().and_then(|items| items.last()) else {
            continue;
        };
        rows.push(ActionStageRow {
            correlation_id: correlation_id.clone(),
            stage: string_field(last, "stage"),
            reason: optional_string(last, "reason"),
        });
    }
    rows
}

fn mention_profiles_from(projections: Option<&Value>) -> BTreeMap<String, MentionProfile> {
    let Some(entries) = projections
        .and_then(|p| p.get("mention_profiles"))
        .and_then(Value::as_object)
    else {
        return BTreeMap::new();
    };
    entries
        .iter()
        .map(|(pubkey, profile)| {
            (
                pubkey.clone(),
                MentionProfile {
                    display: string_field(profile, "display"),
                    picture_url: string_field(profile, "picture_url"),
                    avatar_initials: string_field(profile, "avatar_initials"),
                    avatar_color: string_field(profile, "avatar_color"),
                },
            )
        })
        .collect()
}

fn string_field(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn optional_string(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn number_field(value: &Value, key: &str) -> u64 {
    value.get(key).and_then(Value::as_u64).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_shared_diagnostics_and_action_projections() {
        let payload = serde_json::json!({
            "metrics": {
                "events_rx": 5,
                "visible_items": 2,
                "actor_queue_depth": 1,
                "update_sequence": 9
            },
            "projections": {
                "relay_diagnostics": {
                    "relays": [{
                        "short_url": "relay.example",
                        "role_label": "Read/Write",
                        "connection_label": "Open",
                        "active_sub_count": 3,
                        "total_events_display": "42",
                        "last_event_display": "now",
                        "last_error": null
                    }],
                    "interests": [{
                        "key": "home",
                        "state": "active",
                        "refcount": 1,
                        "cache_coverage": "live"
                    }]
                },
                "action_results": [{
                    "correlation_id": "corr-1",
                    "status": "published",
                    "error": null
                }],
                "action_stages": {
                    "corr-2": [
                        {"stage": "requested", "at_ms": 1},
                        {"stage": "publishing", "at_ms": 2}
                    ]
                },
                "mention_profiles": {
                    "aaaaaaaa": {
                        "display": "Alice",
                        "picture_url": "https://example.com/a.png",
                        "avatar_initials": "AL",
                        "avatar_color": "08E60C"
                    }
                }
            }
        })
        .to_string();

        let snapshot = SharedSnapshot::from_payload(&payload);

        assert_eq!(snapshot.metrics.events_rx, 5);
        assert_eq!(snapshot.relays[0].connection_label, "Open");
        assert_eq!(snapshot.interests[0].cache_coverage, "live");
        assert_eq!(snapshot.action_results[0].correlation_id, "corr-1");
        assert_eq!(snapshot.action_stages[0].stage, "publishing");
        assert_eq!(snapshot.mention_profiles["aaaaaaaa"].display, "Alice");
    }
}
