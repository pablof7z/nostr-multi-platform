//! NIP-88 mint announcement event (kind:38172).
//!
//! A Cashu mint can publish a kind:38172 event to advertise itself on Nostr.
//! The event includes the mint URL, relay preferences, fees, and contact info.
//! Apps use this for **mint discovery** — finding mints via Nostr search
//! rather than hardcoded URLs.
//!
//! Note: this event's own `relays` field is about *mint* reachability, not
//! wallet relay scoping — for the kind:17375 legacy relay hint and why it can
//! never override kind:10019/NIP-65, see the [`crate::wallet_event`] module
//! docs.

use nostr::{EventBuilder, EventId, Kind, Tag, TagKind};

use crate::kinds::KIND_MINT_ANNOUNCE;

/// Decoded kind:38172 mint announcement event.
#[derive(Debug, Clone)]
pub struct MintAnnouncement {
    pub event_id: EventId,
    /// Mint URL (the `d` tag — serves as the identifier for this replaceable event).
    pub mint_url: String,
    /// Relay URLs where this mint is reachable / wants wallet events delivered.
    pub relays: Vec<String>,
    /// Human-readable mint name.
    pub name: Option<String>,
    /// Short description.
    pub description: Option<String>,
    /// Nostr pubkey of the mint operator.
    pub pubkey: Option<String>,
}

/// Build a kind:38172 mint announcement event.
///
/// Mints publish this to advertise themselves on Nostr.
pub fn build_mint_announce_event(
    mint_url: &str,
    relays: Vec<String>,
    name: Option<&str>,
    description: Option<&str>,
) -> EventBuilder {
    let mut tags: Vec<Tag> = Vec::new();
    // `d` tag is the mint URL (addressable event identifier).
    tags.push(Tag::identifier(mint_url));
    for relay in &relays {
        tags.push(Tag::custom(TagKind::custom("relay"), [relay.as_str()]));
    }
    if let Some(n) = name {
        tags.push(Tag::custom(TagKind::custom("name"), [n]));
    }
    if let Some(d) = description {
        tags.push(Tag::custom(TagKind::custom("description"), [d]));
    }
    EventBuilder::new(Kind::from(KIND_MINT_ANNOUNCE as u16), "").tags(tags)
}

/// Decode a kind:38172 event into a [`MintAnnouncement`].
pub fn decode_mint_announce_event(event: &nostr::Event) -> Option<MintAnnouncement> {
    let mut mint_url = None;
    let mut relays = Vec::new();
    let mut name = None;
    let mut description = None;
    let mut pubkey_tag = None;

    for tag in event.tags.iter() {
        match tag.kind() {
            TagKind::SingleLetter(sl)
                if sl.character == nostr::Alphabet::D && !sl.uppercase =>
            {
                if let Some(v) = tag.content() {
                    mint_url = Some(v.to_owned());
                }
            }
            k if k == TagKind::custom("relay") => {
                if let Some(v) = tag.content() {
                    relays.push(v.to_owned());
                }
            }
            k if k == TagKind::custom("name") => {
                if let Some(v) = tag.content() {
                    name = Some(v.to_owned());
                }
            }
            k if k == TagKind::custom("description") => {
                if let Some(v) = tag.content() {
                    description = Some(v.to_owned());
                }
            }
            k if k == TagKind::custom("pubkey") => {
                if let Some(v) = tag.content() {
                    pubkey_tag = Some(v.to_owned());
                }
            }
            _ => {}
        }
    }

    Some(MintAnnouncement {
        event_id: event.id,
        mint_url: mint_url?,
        relays,
        name,
        description,
        pubkey: pubkey_tag,
    })
}

/// Fetch mint announcements from a relay for a given mint URL.
///
/// Returns a filter that will match kind:38172 events for the mint.
pub fn mint_announce_filter(mint_url: &str) -> nostr::Filter {
    nostr::Filter::new()
        .kind(nostr::Kind::from(KIND_MINT_ANNOUNCE as u16))
        .identifier(mint_url)
}
