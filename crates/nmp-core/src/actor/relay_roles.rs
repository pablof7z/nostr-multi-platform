//! Relay role parsing for `RelayEditRow.role`.

/// True when `role` semantically includes `needle`.
///
/// `both` means read+write only; it does not imply indexer. Composite role
/// strings such as `both,indexer` are tokenized on commas, plus signs, and
/// whitespace.
pub(crate) fn has_role(role: &str, needle: &str) -> bool {
    let n = needle.trim().to_ascii_lowercase();
    role_tokens(role)
        .any(|token| token == n || (token == "both" && matches!(n.as_str(), "read" | "write")))
}

/// Normalize a relay role string into the stored `RelayEditRow.role` form.
pub(crate) fn canonical_relay_role(role: &str) -> Option<String> {
    let mut read = false;
    let mut write = false;
    let mut indexer = false;
    let mut saw_token = false;

    for token in role_tokens(role) {
        saw_token = true;
        match token.as_str() {
            "read" => read = true,
            "write" => write = true,
            "both" => {
                read = true;
                write = true;
            }
            "indexer" => indexer = true,
            _ => return None,
        }
    }

    if !saw_token {
        read = true;
        write = true;
    }

    let mut parts = Vec::new();
    match (read, write) {
        (true, true) => parts.push("both"),
        (true, false) => parts.push("read"),
        (false, true) => parts.push("write"),
        (false, false) => {}
    }
    if indexer {
        parts.push("indexer");
    }
    (!parts.is_empty()).then(|| parts.join(","))
}

fn role_tokens(role: &str) -> impl Iterator<Item = String> + '_ {
    role.split(|c: char| c == ',' || c == '+' || c.is_whitespace())
        .filter(|token| !token.is_empty())
        .map(|token| token.to_ascii_lowercase())
}
