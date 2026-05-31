//! Relay role parsing for `AppRelay.role`.

#[derive(Clone, Debug, serde::Serialize, PartialEq, Eq)]
#[cfg_attr(feature = "codegen-schema", derive(schemars::JsonSchema))]
pub(crate) struct RelayRoleOption {
    pub(crate) value: String,
    pub(crate) label: String,
    pub(crate) tint: String,
    pub(crate) is_default: bool,
}

/// The user-facing NIP-65 + indexer role of a configured relay.
///
/// Modeled as three independent capability flags: the canonical role set is
/// combinatorial (`both,indexer`, `read,indexer`, …). Maps 1:1 onto the
/// legacy composite token strings; `to_canonical_string` round-trips through
/// `canonical_relay_role`.
///
/// Distinct from `nmp_network::RelayRole` (transport-lane discriminator:
/// Content | Indexer | Wallet) — see ADR-0021.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Nip65Role {
    pub read: bool,
    pub write: bool,
    pub indexer: bool,
}

impl Nip65Role {
    /// Default role when adding a relay: read + write, no indexer.
    pub const BOTH: Self = Self {
        read: true,
        write: true,
        indexer: false,
    };

    /// Parse a raw, possibly-composite role string into typed flags.
    /// Empty string defaults to `BOTH`. Returns `None` for unrecognised tokens.
    pub fn parse(raw: &str) -> Option<Self> {
        let mut read = false;
        let mut write = false;
        let mut indexer = false;
        let mut saw_token = false;
        for token in role_tokens(raw) {
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
        if !read && !write && !indexer {
            return None;
        }
        Some(Self {
            read,
            write,
            indexer,
        })
    }

    /// True when this role includes the named lane ("read" | "write" | "indexer").
    /// "both" satisfies both "read" and "write" queries.
    pub fn has(&self, lane: &str) -> bool {
        match lane.trim().to_ascii_lowercase().as_str() {
            "read" => self.read,
            "write" => self.write,
            "indexer" => self.indexer,
            "both" => self.read && self.write,
            _ => false,
        }
    }

    /// Serialize to the canonical wire string.
    /// Returns `None` when no flag is set.
    pub fn to_canonical_string(self) -> Option<String> {
        let mut parts = Vec::new();
        match (self.read, self.write) {
            (true, true) => parts.push("both"),
            (true, false) => parts.push("read"),
            (false, true) => parts.push("write"),
            (false, false) => {}
        }
        if self.indexer {
            parts.push("indexer");
        }
        (!parts.is_empty()).then(|| parts.join(","))
    }
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

/// True when `role` semantically includes `needle`.
///
/// `both` means read+write only; it does not imply indexer. Composite role
/// strings such as `both,indexer` are tokenized on commas, plus signs, and
/// whitespace.
pub fn has_role(role: &str, needle: &str) -> bool {
    Nip65Role::parse(role).is_some_and(|r| r.has(needle))
}

/// Normalize a relay role string into the stored `AppRelay.role` form.
#[must_use]
pub(crate) fn canonical_relay_role(role: &str) -> Option<String> {
    Nip65Role::parse(role).and_then(|r| r.to_canonical_string())
}

fn role_tokens(role: &str) -> impl Iterator<Item = String> + '_ {
    role.split(|c: char| c == ',' || c == '+' || c.is_whitespace())
        .filter(|token| !token.is_empty())
        .map(str::to_ascii_lowercase)
}

/// Choose the relay for a client-initiated NIP-46 `nostrconnect://` flow
/// from the user's configured relay rows.
///
/// Returns the first write-capable relay URL, or `None` when no write relay
/// is configured. The caller is responsible for supplying a host-registered
/// bootstrap relay when `None` is returned — nmp-core holds no hardcoded
/// fallback URL (V-65 / D0).
pub fn nostrconnect_relay_url<'a, I>(rows: I) -> Option<String>
where
    I: IntoIterator<Item = (&'a str, &'a str)>,
{
    rows.into_iter()
        .find(|(_, role)| has_role(role, "write"))
        .map(|(url, _)| url.to_string())
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
            Some("wss://write.example".to_string()),
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
            Some("wss://composite.example".to_string()),
            "both,indexer semantically includes write"
        );
    }

    #[test]
    fn nostrconnect_returns_none_without_write_relay() {
        let rows = [
            ("wss://read.example", "read"),
            ("wss://indexer.example", "indexer"),
        ];

        // V-65: no hardcoded fallback — the caller supplies a host-registered
        // bootstrap relay when the user has no configured write relay.
        assert_eq!(nostrconnect_relay_url(rows), None);
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
}
