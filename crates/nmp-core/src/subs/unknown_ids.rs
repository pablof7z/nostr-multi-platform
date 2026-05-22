//! `UnknownIds` — discovery of referenced-but-missing pubkeys and event ids.
//!
//! Ported from notedeck's `UnknownIds` distillation in
//! `docs/design/nostrdb-notedeck-lessons.md` §3.10: while ingesting events the
//! kernel collects referenced pubkeys (`p`-tags, author mentions) and event
//! ids (`e`-tags, `q`-tags) that are **not** in the store, deduplicates them at
//! insertion time, and exposes a drainable set the actor turns into
//! [`crate::subs::OneshotApi`] fetches.
//!
//! Reference scope (documented here so the seam is discoverable):
//! raw NIP-01 tag forms only —
//! - `p` tag position 1 → referenced pubkey,
//! - `e` / `q` tag position 1 → referenced event id.
//!
//! `nevent`/`naddr` bech32 pointers embedded in content are intentionally out
//! of scope: that codec lives in `nmp-nip19` and decoding content is not a
//! `nmp-core` concern. `a`-tag address coordinates are *not* collected here —
//! address-pointer hydration is the planner's `InterestShape::addresses`
//! field, a separate seam left untouched by this module.
//!
//! Doctrine:
//! - **D8** the collect path (`visit_tags`) performs **zero per-event
//!   allocation** when every referenced id is already known: the caller's
//!   `has_*` predicates borrow `&str` straight off the event tags and an id is
//!   only `to_string()`-ed into the set when it is genuinely missing *and* not
//!   already pending. A `|_| true` predicate keeps the set empty (asserted in
//!   tests).
//! - **D6** no panics, no `Result`; the collector is infallible internal
//!   state. Nothing here crosses FFI.
//! - **D4** `UnknownIds` is plain owned state on the kernel actor; the actor
//!   remains the single writer.

use std::collections::BTreeSet;

use crate::planner::interest::EventId;
use crate::planner::Pubkey;

/// Insertion-time-deduplicated set of referenced-but-missing ids.
///
/// Two disjoint sets (pubkeys vs event ids) so the actor can shape distinct
/// oneshot filters (`kinds:[0]` for profiles, id-filters for events). Both use
/// `BTreeSet` so [`UnknownIds::drain`] yields a deterministic order (D8 — plan
/// stability when the actor turns drained ids into interests).
#[derive(Default, Debug)]
pub struct UnknownIds {
    pubkeys: BTreeSet<Pubkey>,
    event_ids: BTreeSet<EventId>,
}

impl UnknownIds {
    /// Empty collector.
    pub fn new() -> Self {
        Self::default()
    }

    /// Borrowed-visitor collect path (D8). Walks `tags` (the raw NIP-01
    /// `Vec<Vec<String>>` shape) and records, for each reference, the id **iff**
    /// the caller's predicate reports it absent from the store.
    ///
    /// - `p` tag → `has_pubkey(pk)`; recorded into the pubkey set when `false`.
    /// - `e` / `q` tag → `has_event(id)`; recorded into the event-id set when
    ///   `false`.
    ///
    /// The predicates receive a borrowed `&str` (no allocation); a `String` is
    /// only materialised on the missing-and-not-yet-pending path. Passing
    /// `|_| true` for both predicates is a guaranteed no-op (the D8 fast path).
    pub fn visit_tags<H, P>(&mut self, tags: &[Vec<String>], has_event: H, has_pubkey: P)
    where
        H: Fn(&str) -> bool,
        P: Fn(&str) -> bool,
    {
        for tag in tags {
            let Some(key) = tag.first().map(String::as_str) else {
                continue;
            };
            let Some(value) = tag.get(1).map(String::as_str) else {
                continue;
            };
            match key {
                "e" | "q" => {
                    if !is_hex64(value) {
                        continue;
                    }
                    // Borrowed checks first — no allocation when known or
                    // already pending (D8).
                    if self.event_ids.contains(value) || has_event(value) {
                        continue;
                    }
                    self.event_ids.insert(value.to_string());
                }
                "p" => {
                    if !is_hex64(value) {
                        continue;
                    }
                    if self.pubkeys.contains(value) || has_pubkey(value) {
                        continue;
                    }
                    self.pubkeys.insert(value.to_string());
                }
                _ => {}
            }
        }
    }

    /// Record a single referenced event id if missing (e.g. an author's own
    /// quoted-note id pulled from content by a higher layer). Same dedup +
    /// borrowed-predicate discipline as [`Self::visit_tags`].
    pub fn note_event<H>(&mut self, id: &str, has_event: H)
    where
        H: Fn(&str) -> bool,
    {
        if !is_hex64(id) || self.event_ids.contains(id) || has_event(id) {
            return;
        }
        self.event_ids.insert(id.to_string());
    }

    /// Record a single referenced pubkey if missing. Mirror of
    /// [`Self::note_event`] for the pubkey set.
    pub fn note_pubkey<P>(&mut self, pk: &str, has_pubkey: P)
    where
        P: Fn(&str) -> bool,
    {
        if !is_hex64(pk) || self.pubkeys.contains(pk) || has_pubkey(pk) {
            return;
        }
        self.pubkeys.insert(pk.to_string());
    }

    /// Drain every pending unknown id, emptying the collector. Returns the
    /// `(event_ids, pubkeys)` pair in deterministic order. **Idempotent**: a
    /// second call with no intervening `visit_*`/`note_*` returns two empty
    /// vecs (the collector is cleared, not errored).
    pub fn drain(&mut self) -> (Vec<EventId>, Vec<Pubkey>) {
        let events: BTreeSet<EventId> = std::mem::take(&mut self.event_ids);
        let pubkeys: BTreeSet<Pubkey> = std::mem::take(&mut self.pubkeys);
        (events.into_iter().collect(), pubkeys.into_iter().collect())
    }

    /// Number of pending unknown ids (event ids + pubkeys). Diagnostics/tests.
    pub fn pending_len(&self) -> usize {
        self.event_ids.len() + self.pubkeys.len()
    }

    /// True when nothing is pending.
    pub fn is_empty(&self) -> bool {
        self.event_ids.is_empty() && self.pubkeys.is_empty()
    }
}

/// True iff `s` is exactly 64 lowercase/uppercase hex chars (a Nostr id or
/// pubkey). Cheap borrowed check — keeps malformed tag values out of the set
/// so a drained oneshot never builds an invalid filter (D6: no downstream
/// surprise).
fn is_hex64(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

impl UnknownIds {
    /// Re-insert event ids that were drained but not yet issued as REQs.
    /// Called by the kernel when it can only open a subset of batches this tick.
    pub fn put_back_events(&mut self, ids: impl IntoIterator<Item = EventId>) {
        self.event_ids.extend(ids);
    }

    /// Re-insert pubkeys that were drained but not yet issued as REQs.
    pub fn put_back_pubkeys(&mut self, pks: impl IntoIterator<Item = Pubkey>) {
        self.pubkeys.extend(pks);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|s| s.to_string()).collect()
    }

    const ID_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const ID_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const PK_C: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

    #[test]
    fn collects_missing_e_and_p_tags() {
        let mut u = UnknownIds::new();
        let tags = vec![
            tag(&["e", ID_A]),
            tag(&["p", PK_C]),
            tag(&["q", ID_B]),
        ];
        u.visit_tags(&tags, |_| false, |_| false);
        let (events, pubkeys) = u.drain();
        assert_eq!(events, vec![ID_A.to_string(), ID_B.to_string()]);
        assert_eq!(pubkeys, vec![PK_C.to_string()]);
    }

    #[test]
    fn known_ids_are_not_collected_and_do_not_allocate() {
        let mut u = UnknownIds::new();
        let tags = vec![tag(&["e", ID_A]), tag(&["p", PK_C])];
        // `|_| true` ⇒ everything is "known" ⇒ D8 fast path, set stays empty.
        u.visit_tags(&tags, |_| true, |_| true);
        assert!(u.is_empty(), "no allocation/insert when all ids are known");
    }

    #[test]
    fn insertion_time_dedup_across_events() {
        let mut u = UnknownIds::new();
        u.visit_tags(&[tag(&["e", ID_A])], |_| false, |_| false);
        u.visit_tags(&[tag(&["e", ID_A])], |_| false, |_| false);
        u.visit_tags(&[tag(&["e", ID_A]), tag(&["e", ID_B])], |_| false, |_| false);
        assert_eq!(u.pending_len(), 2, "ID_A deduped, ID_B added once");
    }

    #[test]
    fn drain_is_idempotent() {
        let mut u = UnknownIds::new();
        u.visit_tags(&[tag(&["e", ID_A])], |_| false, |_| false);
        let first = u.drain();
        assert_eq!(first.0.len(), 1);
        let second = u.drain();
        assert!(second.0.is_empty() && second.1.is_empty(), "second drain empty, not errored");
        assert!(u.is_empty());
    }

    #[test]
    fn malformed_tag_values_are_rejected() {
        let mut u = UnknownIds::new();
        u.visit_tags(
            &[
                tag(&["e", "not-hex"]),
                tag(&["e"]),               // missing value
                tag(&["p", "tooshort"]),
                tag(&["e", &"z".repeat(64)]), // 64 chars but not hex
            ],
            |_| false,
            |_| false,
        );
        assert!(u.is_empty());
    }

    #[test]
    fn note_helpers_dedup_and_respect_predicate() {
        let mut u = UnknownIds::new();
        u.note_event(ID_A, |_| false);
        u.note_event(ID_A, |_| false); // dedup
        u.note_event(ID_B, |_| true); // known ⇒ skipped
        u.note_pubkey(PK_C, |_| false);
        assert_eq!(u.pending_len(), 2);
        let (events, pubkeys) = u.drain();
        assert_eq!(events, vec![ID_A.to_string()]);
        assert_eq!(pubkeys, vec![PK_C.to_string()]);
    }
}
