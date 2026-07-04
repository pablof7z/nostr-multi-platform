//! kind:38172 — NIP-87 Cashu mint announcement codec.
//!
//! A Cashu mint (or anyone re-announcing one) publishes a kind:38172 event to
//! advertise a mint on Nostr: its URLs, supported NUTs/units, and optional
//! human-readable metadata. Apps use these for **mint discovery** — finding
//! mints via Nostr rather than a hardcoded URL list.
//!
//! kind:38172 is addressable (NIP-01 30000–39999): the `(author, kind, d)`
//! tuple identifies one announcement, and [`MintAnnouncement::coordinate`]
//! renders the `38172:<author>:<d>` string a recommendation's `a` tag points
//! at. This crate performs no relay I/O; the kernel fetches these events and
//! feeds their raw parts to [`decode_mint_announcement`].

use nostr::{EventBuilder, Kind, Tag, TagKind};

use crate::capabilities::{parse_capabilities, MintCapabilities};
use crate::kinds::KIND_MINT_ANNOUNCE;

/// Decoded kind:38172 Cashu mint announcement.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MintAnnouncement {
    /// Event id (hex).
    pub event_id: String,
    /// Announcing author's pubkey (hex) — part of the addressable coordinate.
    pub author: String,
    /// The `d` tag: the addressable identifier for this mint announcement (per
    /// NIP-87 the mint's pubkey for Cashu; some announcers use the mint URL).
    pub d_identifier: String,
    /// Mint URLs the wallet can reach (`u` tags, plus the `d` value when it is
    /// itself an `http(s)` URL).
    pub mint_urls: Vec<String>,
    /// Relay URLs advertised for the mint (`relay` tags).
    pub relays: Vec<String>,
    /// Networks advertised (`n` tags, e.g. `mainnet`).
    pub networks: Vec<String>,
    /// Human-readable mint name.
    pub name: Option<String>,
    /// Short description.
    pub description: Option<String>,
    /// Parsed supported NUTs + units (see [`crate::capabilities`]).
    pub capabilities: MintCapabilities,
}

impl MintAnnouncement {
    /// The addressable coordinate `38172:<author>:<d>` a kind:38000
    /// recommendation's `a` tag references.
    #[must_use]
    pub fn coordinate(&self) -> String {
        format!("{KIND_MINT_ANNOUNCE}:{}:{}", self.author, self.d_identifier)
    }

    /// The primary mint URL to use — the first `u` tag, else the `d` value when
    /// it looks like a URL.
    #[must_use]
    pub fn primary_url(&self) -> Option<&str> {
        self.mint_urls.first().map(String::as_str)
    }
}

/// Fields for [`build_mint_announcement`].
#[derive(Clone, Debug, Default)]
pub struct MintAnnouncementDraft<'a> {
    /// The `d` identifier (mint pubkey or URL).
    pub d_identifier: &'a str,
    /// Mint URLs (`u` tags).
    pub mint_urls: &'a [String],
    /// Relay URLs (`relay` tags).
    pub relays: &'a [String],
    /// Networks (`n` tags).
    pub networks: &'a [String],
    /// Supported NUT numbers, rendered as a single comma-separated `nuts` tag.
    pub nuts: &'a [u16],
    /// Human-readable name.
    pub name: Option<&'a str>,
    /// Short description.
    pub description: Option<&'a str>,
    /// Optional NUT-06 `GetInfo` JSON to place in `content`.
    pub content: &'a str,
}

/// Build a kind:38172 Cashu mint announcement event.
#[must_use]
pub fn build_mint_announcement(draft: &MintAnnouncementDraft<'_>) -> EventBuilder {
    let mut tags: Vec<Tag> = Vec::new();
    tags.push(Tag::identifier(draft.d_identifier));
    for url in draft.mint_urls {
        tags.push(Tag::custom(TagKind::custom("u"), [url.as_str()]));
    }
    for relay in draft.relays {
        tags.push(Tag::custom(TagKind::custom("relay"), [relay.as_str()]));
    }
    for network in draft.networks {
        tags.push(Tag::custom(TagKind::custom("n"), [network.as_str()]));
    }
    if !draft.nuts.is_empty() {
        let list = draft
            .nuts
            .iter()
            .map(u16::to_string)
            .collect::<Vec<_>>()
            .join(",");
        tags.push(Tag::custom(TagKind::custom("nuts"), [list]));
    }
    if let Some(name) = draft.name {
        tags.push(Tag::custom(TagKind::custom("name"), [name]));
    }
    if let Some(description) = draft.description {
        tags.push(Tag::custom(TagKind::custom("description"), [description]));
    }
    EventBuilder::new(Kind::from(KIND_MINT_ANNOUNCE as u16), draft.content).tags(tags)
}

/// Decode a kind:38172 announcement from its raw parts (the shape observed
/// events arrive in). Returns `None` when the mandatory `d` identifier is
/// absent — an addressable event without one has no stable identity.
#[must_use]
pub fn decode_mint_announcement(
    event_id: &str,
    author: &str,
    tags: &[Vec<String>],
    content: &str,
) -> Option<MintAnnouncement> {
    let mut d_identifier: Option<String> = None;
    let mut mint_urls: Vec<String> = Vec::new();
    let mut relays: Vec<String> = Vec::new();
    let mut networks: Vec<String> = Vec::new();
    let mut name: Option<String> = None;
    let mut description: Option<String> = None;

    for tag in tags {
        let Some(key) = tag.first() else { continue };
        let value = tag.get(1).map(String::as_str);
        match key.as_str() {
            "d" => d_identifier = value.map(str::to_string),
            "u" => {
                if let Some(v) = value {
                    mint_urls.push(v.to_string());
                }
            }
            "relay" => {
                if let Some(v) = value {
                    relays.push(v.to_string());
                }
            }
            "n" => {
                if let Some(v) = value {
                    networks.push(v.to_string());
                }
            }
            "name" => name = value.map(str::to_string),
            "description" => description = value.map(str::to_string),
            _ => {}
        }
    }

    let d_identifier = d_identifier?;
    // A `d` that is itself a mint URL doubles as a usable endpoint when no
    // explicit `u` tag was provided (older announcements used `d` = URL).
    if mint_urls.is_empty() && looks_like_url(&d_identifier) {
        mint_urls.push(d_identifier.clone());
    }

    Some(MintAnnouncement {
        event_id: event_id.to_string(),
        author: author.to_string(),
        d_identifier,
        mint_urls,
        relays,
        networks,
        name,
        description,
        capabilities: parse_capabilities(tags, content),
    })
}

/// Convenience decoder over a signed `nostr::Event`.
#[must_use]
pub fn decode_mint_announcement_event(event: &nostr::Event) -> Option<MintAnnouncement> {
    if event.kind != Kind::from(KIND_MINT_ANNOUNCE as u16) {
        return None;
    }
    decode_mint_announcement(
        &event.id.to_hex(),
        &event.pubkey.to_hex(),
        &raw_tags(event),
        &event.content,
    )
}

/// Filter matching every kind:38172 announcement (mint discovery is a
/// whole-kind read; the reading account narrows by web-of-trust in Rust).
#[must_use]
pub fn mint_announcement_filter() -> nostr::Filter {
    nostr::Filter::new().kind(Kind::from(KIND_MINT_ANNOUNCE as u16))
}

fn looks_like_url(value: &str) -> bool {
    value.starts_with("http://") || value.starts_with("https://")
}

pub(crate) fn raw_tags(event: &nostr::Event) -> Vec<Vec<String>> {
    event
        .tags
        .iter()
        .map(|tag| tag.clone().to_vec())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use nostr::Keys;

    fn sign(builder: EventBuilder) -> nostr::Event {
        builder.sign_with_keys(&Keys::generate()).expect("sign")
    }

    #[test]
    fn round_trips_through_a_signed_event() {
        let urls = vec!["https://mint.example".to_string()];
        let relays = vec!["wss://relay.example".to_string()];
        let draft = MintAnnouncementDraft {
            d_identifier: "mintpubkeyhex",
            mint_urls: &urls,
            relays: &relays,
            networks: &["mainnet".to_string()],
            nuts: &[4, 7, 11, 12],
            name: Some("Example Mint"),
            description: Some("a test mint"),
            content: "",
        };
        let event = sign(build_mint_announcement(&draft));
        let decoded = decode_mint_announcement_event(&event).expect("decodes");
        assert_eq!(decoded.d_identifier, "mintpubkeyhex");
        assert_eq!(decoded.mint_urls, urls);
        assert_eq!(decoded.relays, relays);
        assert_eq!(decoded.networks, vec!["mainnet".to_string()]);
        assert_eq!(decoded.name.as_deref(), Some("Example Mint"));
        assert!(decoded.capabilities.supports_nutzap());
        assert_eq!(decoded.coordinate(), format!("38172:{}:mintpubkeyhex", event.pubkey.to_hex()));
    }

    #[test]
    fn missing_d_tag_is_rejected() {
        assert!(decode_mint_announcement("id", "author", &[], "").is_none());
        let tags = vec![vec!["u".to_string(), "https://mint.example".to_string()]];
        assert!(decode_mint_announcement("id", "author", &tags, "").is_none());
    }

    #[test]
    fn wrong_kind_event_is_rejected() {
        let event = sign(EventBuilder::new(Kind::from(1u16), "hi"));
        assert!(decode_mint_announcement_event(&event).is_none());
    }

    #[test]
    fn d_url_doubles_as_mint_url_when_no_u_tag() {
        let tags = vec![vec!["d".to_string(), "https://legacy.mint".to_string()]];
        let decoded = decode_mint_announcement("id", "author", &tags, "").expect("decodes");
        assert_eq!(decoded.mint_urls, vec!["https://legacy.mint".to_string()]);
    }
}
