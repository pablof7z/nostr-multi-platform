use serde_json::{json, Value};

use crate::app::AppRuntime;
use crate::Result;

pub fn init(app: &mut AppRuntime, relays: &[String]) -> Result<Value> {
    app.register_active_marmot()?;
    publish_key_package(app, relays)
}

pub fn publish_key_package(app: &AppRuntime, relays: &[String]) -> Result<Value> {
    app.marmot_dispatch(json!({
        "op": "publish_key_package",
        "relays": relays,
    }))
}

pub fn create_group(
    app: &AppRuntime,
    name: &str,
    invitees: &[String],
    relays: &[String],
) -> Result<Value> {
    app.marmot_dispatch(json!({
        "op": "create_group",
        "name": name,
        "relays": relays,
        "invitee_npubs": invitees,
    }))
}

pub fn invite(app: &AppRuntime, group_id: &str, invitees: &[String]) -> Result<Value> {
    app.marmot_dispatch(json!({
        "op": "invite",
        "group_id_hex": group_id,
        "invitee_npubs": invitees,
    }))
}

pub fn accept(app: &AppRuntime, welcome_id: &str) -> Result<Value> {
    app.marmot_dispatch(json!({
        "op": "accept_welcome",
        "welcome_id_hex": welcome_id,
    }))
}

pub fn send(app: &AppRuntime, group_id: &str, text: &str) -> Result<Value> {
    app.marmot_dispatch(json!({
        "op": "send",
        "group_id_hex": group_id,
        "text": text,
    }))
}

pub fn ingest_event_json(app: &AppRuntime, event_json: &str) -> Result<Value> {
    app.marmot_dispatch(json!({
        "op": "ingest_signed_event",
        "event_json": event_json,
    }))
}

pub fn group_messages(app: &AppRuntime, group_id: &str) -> Result<Value> {
    app.marmot_group_messages(group_id)
}

pub fn first_pending_welcome_id(snapshot: &Value) -> Option<String> {
    snapshot
        .get("pending_welcomes")
        .and_then(Value::as_array)?
        .first()?
        .get("id_hex")
        .and_then(Value::as_str)
        .map(str::to_string)
}

pub fn event_strings(value: &Value, key: &str) -> Vec<String> {
    value
        .get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use nostr::nips::nip19::ToBech32;
    use nostr::Keys;

    use super::*;

    fn nsec(keys: &Keys) -> String {
        keys.secret_key().to_bech32().expect("encode nsec")
    }

    #[test]
    fn chirp_repl_mls_end_to_end_uses_shared_marmot_runtime() {
        let relays = vec!["ws://127.0.0.1:9".to_string()];
        let alice_keys = Keys::generate();
        let bob_keys = Keys::generate();
        let mut alice = AppRuntime::new();
        let mut bob = AppRuntime::new();

        alice
            .sign_in_nsec_with_marmot(&nsec(&alice_keys))
            .expect("alice marmot identity");
        bob.sign_in_nsec_with_marmot(&nsec(&bob_keys))
            .expect("bob marmot identity");

        let bob_kp = publish_key_package(&bob, &relays).expect("bob publishes key package");
        for event in event_strings(&bob_kp, "events") {
            ingest_event_json(&alice, &event).expect("alice caches bob key package");
        }

        let group = create_group(
            &alice,
            "repl-e2e",
            &[bob_keys.public_key().to_hex()],
            &relays,
        )
        .expect("alice creates MLS group");
        let group_id = group["group_id_hex"]
            .as_str()
            .expect("group id")
            .to_string();
        let welcomes = event_strings(&group, "welcome_rumors");
        assert_eq!(welcomes.len(), 1);

        for welcome in welcomes {
            ingest_event_json(&bob, &welcome).expect("bob ingests welcome");
        }
        let welcome_id = first_pending_welcome_id(&bob.marmot_snapshot().expect("bob snapshot"))
            .expect("pending welcome");
        let accept = accept(&bob, &welcome_id).expect("bob accepts welcome");
        assert_eq!(accept["group_id_hex"], group_id);
        if let Some(event) = accept
            .get("post_join_self_update_event")
            .and_then(Value::as_str)
        {
            ingest_event_json(&alice, event).expect("alice ingests bob self-update");
        }

        let sent = send(&alice, &group_id, "hello from chirp-repl mls")
            .expect("alice sends encrypted group message");
        let message = sent["event"].as_str().expect("message event");
        ingest_event_json(&bob, message).expect("bob ingests group message");

        let rows = group_messages(&bob, &group_id).expect("bob decrypts group messages");
        let rows = rows.as_array().expect("message rows");
        assert!(rows.iter().any(|row| {
            row.get("content").and_then(Value::as_str) == Some("hello from chirp-repl mls")
        }));
    }
}
