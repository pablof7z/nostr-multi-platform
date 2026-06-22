//! `parse_relay_list_to_substrate` — translate the `nostr` relay-list parse into
//! the substrate [`ParsedRelayList`] the [`MailboxCache`](super::MailboxCache)
//! trait operates on. Split out of `mod.rs` to keep that file under the 500-LOC
//! ceiling (AGENTS.md §file-size).

use super::nostr::parse_relay_list;
use super::ParsedRelayList;

/// Translate `parse_relay_list` output into the [`ParsedRelayList`] form the
/// [`MailboxCache`](super::MailboxCache) trait operates on.
///
/// Supersession is enforced by the store before this path is reached; there is
/// no kernel-side belt-and-suspenders mirror (single source of truth per
/// `AGENTS.md`).
pub(crate) fn parse_relay_list_to_substrate(tags: &[Vec<String>]) -> ParsedRelayList {
    let legacy = parse_relay_list(tags);
    ParsedRelayList {
        read: legacy.read_relays,
        write: legacy.write_relays,
        both: legacy.both_relays,
    }
}
