//! NIP-89 client-identity vocabulary. See crate description.

/// A typed `31990:<pubkey-hex>:<d>` NIP-89 handler coordinate, plus an optional
/// relay hint. Validated at construction.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Nip89Handler {
    pubkey: String,     // 64-hex, validated via nostr::PublicKey::from_hex
    identifier: String, // the `d` value; may be empty (NIP-01 default)
    relay_hint: Option<String>,
}

impl Nip89Handler {
    /// Validate and construct. `pubkey` must be 64-hex (rust-nostr parse).
    /// `identifier` may be empty. `relay_hint`, when `Some`, must be non-empty.
    pub fn new(
        pubkey: impl Into<String>,
        identifier: impl Into<String>,
        relay_hint: Option<String>,
    ) -> Result<Self, String> {
        let pubkey = pubkey.into();
        nostr::PublicKey::from_hex(&pubkey)
            .map_err(|e| format!("invalid NIP-89 handler pubkey: {e}"))?;
        if let Some(ref hint) = relay_hint {
            if hint.is_empty() {
                return Err("NIP-89 handler relay hint must be non-empty when present".into());
            }
        }
        Ok(Self {
            pubkey,
            identifier: identifier.into(),
            relay_hint,
        })
    }

    /// `31990:<pubkey>:<d>` — NIP-89 handler kind is fixed at 31990.
    fn coordinate(&self) -> String {
        format!("31990:{}:{}", self.pubkey, self.identifier)
    }
}

/// Single app identity declared once at the composition root.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClientIdentity {
    /// e.g. "Chirp" — feeds the UA AND the client-tag name slot.
    pub name: String,
    /// e.g. "1.2.0" — UA only.
    pub version: Option<String>,
    /// Optional typed NIP-89 handler coordinate (+ optional relay hint).
    pub handler: Option<Nip89Handler>,
}

impl ClientIdentity {
    /// Relay WebSocket User-Agent: `Name/ver (nmp/<ver>)`, or `Name (nmp/<ver>)`
    /// when no version. `<ver>` is this crate's CARGO_PKG_VERSION (= the nmp
    /// workspace version).
    #[must_use]
    pub fn user_agent(&self) -> String {
        let nmp = env!("CARGO_PKG_VERSION");
        match &self.version {
            Some(v) => format!("{}/{} (nmp/{})", self.name, v, nmp),
            None => format!("{} (nmp/{})", self.name, nmp),
        }
    }

    /// NIP-89 `client` tag: `["client", name]`, `["client", name, coord]`, or
    /// `["client", name, coord, relay_hint]`.
    #[must_use]
    pub fn client_tag(&self) -> Vec<String> {
        let mut tag = vec!["client".to_string(), self.name.clone()];
        if let Some(h) = &self.handler {
            tag.push(h.coordinate());
            if let Some(hint) = &h.relay_hint {
                tag.push(hint.clone());
            }
        }
        tag
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID_PUBKEY: &str = "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d";

    #[test]
    fn user_agent_with_version() {
        let identity = ClientIdentity {
            name: "Chirp".to_string(),
            version: Some("1.2.0".to_string()),
            handler: None,
        };
        assert_eq!(
            identity.user_agent(),
            format!("Chirp/1.2.0 (nmp/{})", env!("CARGO_PKG_VERSION"))
        );
    }

    #[test]
    fn user_agent_without_version() {
        let identity = ClientIdentity {
            name: "Chirp".to_string(),
            version: None,
            handler: None,
        };
        assert_eq!(
            identity.user_agent(),
            format!("Chirp (nmp/{})", env!("CARGO_PKG_VERSION"))
        );
    }

    #[test]
    fn client_tag_name_only() {
        let identity = ClientIdentity {
            name: "Chirp".to_string(),
            version: Some("1.2.0".to_string()),
            handler: None,
        };
        assert_eq!(
            identity.client_tag(),
            vec!["client".to_string(), "Chirp".to_string()]
        );
    }

    #[test]
    fn client_tag_with_coordinate() {
        let handler = Nip89Handler::new(VALID_PUBKEY, "myapp", None).unwrap();
        let identity = ClientIdentity {
            name: "Chirp".to_string(),
            version: Some("1.2.0".to_string()),
            handler: Some(handler),
        };
        let tag = identity.client_tag();
        assert_eq!(tag.len(), 3);
        assert_eq!(tag[0], "client");
        assert_eq!(tag[1], "Chirp");
        assert!(tag[2].starts_with("31990:"));
        assert!(tag[2].contains(VALID_PUBKEY));
        assert!(tag[2].ends_with(":myapp"));
    }

    #[test]
    fn client_tag_with_relay_hint() {
        let handler = Nip89Handler::new(
            VALID_PUBKEY,
            "myapp",
            Some("wss://relay.example".to_string()),
        )
        .unwrap();
        let identity = ClientIdentity {
            name: "Chirp".to_string(),
            version: Some("1.2.0".to_string()),
            handler: Some(handler),
        };
        let tag = identity.client_tag();
        assert_eq!(tag.len(), 4);
        assert_eq!(tag[0], "client");
        assert_eq!(tag[1], "Chirp");
        assert!(tag[2].starts_with("31990:"));
        assert_eq!(tag[3], "wss://relay.example");
    }

    #[test]
    fn nip89_handler_rejects_bad_pubkey() {
        let result = Nip89Handler::new("not-hex", "d", None);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("invalid"));
    }

    #[test]
    fn nip89_handler_rejects_short_pubkey() {
        let result = Nip89Handler::new("abcd", "d", None);
        assert!(result.is_err());
    }

    #[test]
    fn nip89_handler_accepts_empty_d() {
        let handler = Nip89Handler::new(VALID_PUBKEY, "", None).unwrap();
        let coord = handler.coordinate();
        assert!(coord.starts_with("31990:"));
        assert!(coord.ends_with(":"));
    }

    #[test]
    fn nip89_handler_rejects_empty_relay_hint() {
        let result = Nip89Handler::new(VALID_PUBKEY, "d", Some("".to_string()));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("non-empty"));
    }
}
