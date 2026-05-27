use crate::app::{AppRuntime, AppState};

pub(super) const CREATE_GROUP_ACTION: &str = "create-group";

pub(super) fn start_create_group(state: &mut AppState) {
    state.start_modal(
        "Create group",
        vec![
            "Protocol (nip29/mls)",
            "Name",
            "Relay(s)",
            "NIP-29 local id",
            "MLS invitees",
        ],
        CREATE_GROUP_ACTION,
    );
}

pub(super) fn dispatch_create_group(
    fields: &[(String, String)],
    state: &mut AppState,
    runtime: &AppRuntime,
) {
    let protocol = field(fields, 0)
        .unwrap_or("nip29")
        .trim()
        .to_ascii_lowercase();
    let name = field(fields, 1).unwrap_or("").trim();
    let relays = list_tokens(field(fields, 2).unwrap_or(""));

    if name.is_empty() {
        state.push_toast("group name is required");
        return;
    }
    if relays.is_empty() {
        state.push_toast("at least one relay is required");
        return;
    }

    match protocol.as_str() {
        "" | "nip29" | "public" => create_public_group(fields, name, &relays[0], state, runtime),
        "mls" | "marmot" => create_mls_group(fields, name, &relays, state, runtime),
        _ => state.push_toast("protocol must be nip29 or mls"),
    }
}

fn create_public_group(
    fields: &[(String, String)],
    name: &str,
    relay: &str,
    state: &mut AppState,
    runtime: &AppRuntime,
) {
    let local_id = field(fields, 3).unwrap_or("").trim();
    if local_id.is_empty() {
        state.push_toast("NIP-29 local id is required");
        return;
    }
    match runtime.create_public_group(relay, local_id, name, None) {
        Ok(cid) => state.track_action(cid, &format!("group create {local_id}")),
        Err(e) => state.push_toast(&format!("group create failed: {e}")),
    }
}

fn create_mls_group(
    fields: &[(String, String)],
    name: &str,
    relays: &[String],
    state: &mut AppState,
    runtime: &AppRuntime,
) {
    let invitee_text = field(fields, 4)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    match runtime.marmot_create_group(name, relays, invitee_text) {
        Ok(result) => state.status = format!("mls {}", truncate(&result)),
        Err(e) => state.push_toast(&format!("mls create failed: {e}")),
    }
}

fn field(fields: &[(String, String)], index: usize) -> Option<&str> {
    fields.get(index).map(|(_, value)| value.as_str())
}

fn list_tokens(value: &str) -> Vec<String> {
    value
        .split(|c: char| c.is_whitespace() || c == ',' || c == ';')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect()
}

fn truncate(value: &str) -> String {
    let compact = value.replace('\n', " ");
    if compact.chars().count() <= 120 {
        compact
    } else {
        format!("{}...", compact.chars().take(117).collect::<String>())
    }
}

#[cfg(test)]
mod tests {
    use super::list_tokens;

    #[test]
    fn list_tokens_accepts_commas_semicolons_and_spaces() {
        assert_eq!(
            list_tokens("wss://a, wss://b;ws://c  wss://d"),
            vec!["wss://a", "wss://b", "ws://c", "wss://d"]
        );
    }
}
