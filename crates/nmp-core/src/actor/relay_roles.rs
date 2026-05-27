//! Relay role parsing for `RelayEditRow.role`.

/// Fallback relay for client-initiated NIP-46 `nostrconnect://` handshakes
/// when the user has no configured write relay.
pub const NOSTRCONNECT_DEFAULT_RELAY_URL: &str = "wss://relay.damus.io";

#[derive(Clone, Debug, serde::Serialize, PartialEq, Eq)]
#[cfg_attr(feature = "codegen-schema", derive(schemars::JsonSchema))]
pub(crate) struct RelayRoleOption {
    pub(crate) value: String,
    pub(crate) label: String,
    pub(crate) tint: String,
    pub(crate) is_default: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RelayRoleMetadata {
    value: &'static str,
    label: &'static str,
    tint: &'static str,
    is_default: bool,
}

const RELAY_ROLE_METADATA: &[RelayRoleMetadata] = &[
    RelayRoleMetadata {
        value: "both,indexer",
        label: "Both + Index",
        tint: "accent",
        is_default: false,
    },
    RelayRoleMetadata {
        value: "both",
        label: "Both",
        tint: "accent",
        is_default: true,
    },
    RelayRoleMetadata {
        value: "read",
        label: "Read",
        tint: "info",
        is_default: false,
    },
    RelayRoleMetadata {
        value: "write",
        label: "Write",
        tint: "success",
        is_default: false,
    },
    RelayRoleMetadata {
        value: "indexer",
        label: "Index",
        tint: "neutral",
        is_default: false,
    },
];

#[must_use]
pub(crate) fn relay_role_options() -> Vec<RelayRoleOption> {
    RELAY_ROLE_METADATA
        .iter()
        .map(|metadata| RelayRoleOption {
            value: metadata.value.to_string(),
            label: metadata.label.to_string(),
            tint: metadata.tint.to_string(),
            is_default: metadata.is_default,
        })
        .collect()
}

pub(crate) fn relay_role_label(role: &str) -> String {
    role_metadata(role).map_or(role, |m| m.label).to_string()
}

pub(crate) fn relay_role_tint(role: &str) -> String {
    role_metadata(role).map_or("accent", |m| m.tint).to_string()
}

/// True when `role` semantically includes `needle`.
///
/// `both` means read+write only; it does not imply indexer. Composite role
/// strings such as `both,indexer` are tokenized on commas, plus signs, and
/// whitespace.
pub fn has_role(role: &str, needle: &str) -> bool {
    let n = needle.trim().to_ascii_lowercase();
    role_tokens(role)
        .any(|token| token == n || (token == "both" && matches!(n.as_str(), "read" | "write")))
}

/// Normalize a relay role string into the stored `RelayEditRow.role` form.
#[must_use]
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
        .map(str::to_ascii_lowercase)
}

/// Choose the relay for a client-initiated NIP-46 `nostrconnect://` flow.
pub fn nostrconnect_relay_url<'a, I>(rows: I) -> String
where
    I: IntoIterator<Item = (&'a str, &'a str)>,
{
    rows.into_iter()
        .find(|(_, role)| has_role(role, "write"))
        .map_or_else(
            || NOSTRCONNECT_DEFAULT_RELAY_URL.to_string(),
            |(url, _)| url.to_string(),
        )
}

fn role_metadata(role: &str) -> Option<&'static RelayRoleMetadata> {
    let canonical = canonical_relay_role(role)?;
    RELAY_ROLE_METADATA
        .iter()
        .find(|metadata| metadata.value == canonical)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nostrconnect_prefers_first_write_eligible_relay() {
        let rows = [
            ("wss://read.example", "read"),
            ("wss://write.example", "write"),
            ("wss://both.example", "both"),
        ];

        assert_eq!(
            nostrconnect_relay_url(rows),
            "wss://write.example",
            "first write-capable relay should own nostrconnect handshakes"
        );
    }

    #[test]
    fn nostrconnect_accepts_composite_role_tokens() {
        let rows = [
            ("wss://indexer.example", "indexer"),
            ("wss://composite.example", "both,indexer"),
        ];

        assert_eq!(
            nostrconnect_relay_url(rows),
            "wss://composite.example",
            "both,indexer semantically includes write"
        );
    }

    #[test]
    fn nostrconnect_falls_back_without_write_relay() {
        let rows = [
            ("wss://read.example", "read"),
            ("wss://indexer.example", "indexer"),
        ];

        assert_eq!(nostrconnect_relay_url(rows), NOSTRCONNECT_DEFAULT_RELAY_URL);
    }

    #[test]
    fn role_options_are_projection_ready() {
        let options = relay_role_options();
        let values = options
            .iter()
            .map(|option| option.value.as_str())
            .collect::<Vec<_>>();
        assert_eq!(values, ["both,indexer", "both", "read", "write", "indexer"]);
        assert_eq!(
            options
                .iter()
                .filter(|option| option.is_default)
                .map(|option| option.value.as_str())
                .collect::<Vec<_>>(),
            ["both"]
        );
        assert_eq!(options[0].label, "Both + Index");
        assert_eq!(options[0].tint, "accent");
    }

    #[test]
    fn role_display_metadata_uses_canonical_roles() {
        assert_eq!(relay_role_label("write read indexer"), "Both + Index");
        assert_eq!(relay_role_tint("READ"), "info");
        assert_eq!(relay_role_tint("write"), "success");
        assert_eq!(relay_role_label("indexer"), "Index");
    }
}
