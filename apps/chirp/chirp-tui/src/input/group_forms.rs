use crate::app::{AppRuntime, AppState};

pub(super) const CREATE_GROUP_ACTION: &str = "create-group";

pub(super) fn start_create_group(state: &mut AppState) {
    state.start_modal(
        "Create group",
        vec!["Protocol (nip29/mls)", "Name", "Relay(s)", "MLS invitees"],
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
        "" | "nip29" | "public" => create_public_group(name, &relays[0], state, runtime),
        "mls" | "marmot" => create_mls_group(fields, name, &relays, state, runtime),
        _ => state.push_toast("protocol must be nip29 or mls"),
    }
}

fn create_public_group(name: &str, relay: &str, state: &mut AppState, runtime: &AppRuntime) {
    let local_id = generated_local_id(name);
    match runtime.create_public_group(relay, &local_id, name, None) {
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
    let invitee_text = field(fields, 3)
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

fn generated_local_id(name: &str) -> String {
    local_id_with_suffix(name, fastrand::u32(100_000..1_000_000))
}

fn local_id_with_suffix(name: &str, suffix: u32) -> String {
    format!("{}-{suffix}", slug_title(name))
}

fn slug_title(name: &str) -> String {
    let mut slug = String::new();
    let mut last_was_dash = true;

    for ch in name.chars().flat_map(char::to_lowercase) {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch);
            last_was_dash = false;
        } else if !last_was_dash {
            slug.push('-');
            last_was_dash = true;
        }
    }

    while slug.ends_with('-') {
        slug.pop();
    }

    if slug.is_empty() {
        "group".to_string()
    } else {
        slug
    }
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
    use super::{list_tokens, local_id_with_suffix, slug_title};

    #[test]
    fn list_tokens_accepts_commas_semicolons_and_spaces() {
        assert_eq!(
            list_tokens("wss://a, wss://b;ws://c  wss://d"),
            vec!["wss://a", "wss://b", "ws://c", "wss://d"]
        );
    }

    #[test]
    fn slug_title_uses_nip29_local_id_charset() {
        assert_eq!(slug_title("Rust Nostr! Dev Room"), "rust-nostr-dev-room");
        assert_eq!(slug_title("  ---  "), "group");
    }

    #[test]
    fn local_id_appends_numeric_suffix() {
        assert_eq!(local_id_with_suffix("Blah", 123_213), "blah-123213");
    }
}
