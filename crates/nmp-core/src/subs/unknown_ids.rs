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

/// Hard cap on the number of pending unknown ids retained **per set**
/// (pubkeys and event ids are capped independently).
///
/// `UnknownIds` is fed straight from untrusted relay traffic: every `p`/`e`/`q`
/// tag on every ingested event that references a missing id lands here until the
/// actor drains it into a oneshot fetch. A hostile or merely chatty relay can
/// emit events that reference thousands of distinct ids faster than the actor
/// can drain them, so without a bound the set grows without limit (D5 — resident
/// state must be capacity-bounded).
///
/// When a set is at capacity a **new** id is dropped rather than evicting an
/// existing one (oldest-first retention). This is the right trade-off for a
/// discovery set: the ids already pending are ones we have committed to
/// fetching; throwing them away to admit flood entries would let an attacker
/// starve real discovery. A dropped id is not lost forever — if it is
/// referenced again after the set has drained below the cap it is re-collected
/// on the next ingest.
pub const MAX_UNKNOWN_IDS: usize = 500;

/// Insertion-time-deduplicated set of referenced-but-missing ids.
///
/// Two disjoint sets (pubkeys vs event ids) so the actor can shape distinct
/// oneshot filters (`kinds:[0]` for profiles, id-filters for events). Both use
/// `BTreeSet` so [`UnknownIds::drain`] yields a deterministic order (D8 — plan
/// stability when the actor turns drained ids into interests).
///
/// Each set is hard-capped at [`MAX_UNKNOWN_IDS`]; see that constant for the
/// flood-resistance rationale.
#[derive(Default, Debug)]
pub struct UnknownIds {
    pubkeys: BTreeSet<Pubkey>,
    event_ids: BTreeSet<EventId>,
}

/// Insert `value` into `set` iff doing so would not exceed [`MAX_UNKNOWN_IDS`].
///
/// Returns `true` when the value ends up in the set (newly inserted *or* already
/// present), `false` when the set was at capacity and `value` was a new id that
/// got dropped. A value already in the set never counts against the cap a second
/// time, so an at-capacity set still accepts re-references to ids it holds.
fn try_insert_capped<T: Ord>(set: &mut BTreeSet<T>, value: T) -> bool {
    if set.contains(&value) {
        return true;
    }
    if set.len() >= MAX_UNKNOWN_IDS {
        return false;
    }
    set.insert(value);
    true
}

impl UnknownIds {
    /// Empty collector.
    #[must_use]
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
                    try_insert_capped(&mut self.event_ids, value.to_string());
                }
                "p" => {
                    if !is_hex64(value) {
                        continue;
                    }
                    if self.pubkeys.contains(value) || has_pubkey(value) {
                        continue;
                    }
                    try_insert_capped(&mut self.pubkeys, value.to_string());
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
        try_insert_capped(&mut self.event_ids, id.to_string());
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
        try_insert_capped(&mut self.pubkeys, pk.to_string());
    }

    /// Drain every pending unknown id, emptying the collector. Returns the
    /// `(event_ids, pubkeys)` pair in deterministic order. **Idempotent**: a
    /// second call with no intervening `visit_*`/`note_*` returns two empty
    /// vecs (the collector is cleared, not errored).
    #[must_use]
    pub fn drain(&mut self) -> (Vec<EventId>, Vec<Pubkey>) {
        let events: BTreeSet<EventId> = std::mem::take(&mut self.event_ids);
        let pubkeys: BTreeSet<Pubkey> = std::mem::take(&mut self.pubkeys);
        (events.into_iter().collect(), pubkeys.into_iter().collect())
    }

    /// Number of pending unknown ids (event ids + pubkeys). Diagnostics/tests.
    #[must_use]
    pub fn pending_len(&self) -> usize {
        self.event_ids.len() + self.pubkeys.len()
    }

    /// True when nothing is pending.
    #[must_use]
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
    ///
    /// Honours [`MAX_UNKNOWN_IDS`]: a put-back that would exceed the cap drops
    /// the surplus ids. In practice the cap is never hit on this path (the
    /// remainder being put back was itself drained from an already-capped set),
    /// but routing every insert through the same gate keeps the bound an
    /// invariant rather than a property of one code path.
    pub fn put_back_events(&mut self, ids: impl IntoIterator<Item = EventId>) {
        for id in ids {
            try_insert_capped(&mut self.event_ids, id);
        }
    }

    /// Re-insert pubkeys that were drained but not yet issued as REQs. Honours
    /// [`MAX_UNKNOWN_IDS`] the same way [`Self::put_back_events`] does.
    pub fn put_back_pubkeys(&mut self, pks: impl IntoIterator<Item = Pubkey>) {
        for pk in pks {
            try_insert_capped(&mut self.pubkeys, pk);
        }
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
        let tags = vec![tag(&["e", ID_A]), tag(&["p", PK_C]), tag(&["q", ID_B])];
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
        u.visit_tags(
            &[tag(&["e", ID_A]), tag(&["e", ID_B])],
            |_| false,
            |_| false,
        );
        assert_eq!(u.pending_len(), 2, "ID_A deduped, ID_B added once");
    }

    #[test]
    fn drain_is_idempotent() {
        let mut u = UnknownIds::new();
        u.visit_tags(&[tag(&["e", ID_A])], |_| false, |_| false);
        let first = u.drain();
        assert_eq!(first.0.len(), 1);
        let second = u.drain();
        assert!(
            second.0.is_empty() && second.1.is_empty(),
            "second drain empty, not errored"
        );
        assert!(u.is_empty());
    }

    #[test]
    fn malformed_tag_values_are_rejected() {
        let mut u = UnknownIds::new();
        u.visit_tags(
            &[
                tag(&["e", "not-hex"]),
                tag(&["e"]), // missing value
                tag(&["p", "tooshort"]),
                tag(&["e", &"z".repeat(64)]), // 64 chars but not hex
            ],
            |_| false,
            |_| false,
        );
        assert!(u.is_empty());
    }

    /// A relay flood that references far more distinct ids than the cap must
    /// not grow either set past [`MAX_UNKNOWN_IDS`], and the ids collected
    /// *first* must survive (oldest-first retention — new entries are dropped at
    /// capacity, existing ones are kept).
    #[test]
    fn flood_is_capped_and_keeps_first_inserted() {
        let mut u = UnknownIds::new();
        let n = MAX_UNKNOWN_IDS + 50;

        // 64-hex ids whose lexicographic order matches insertion order, so the
        // BTreeSet's stored order is the insertion order and "first inserted
        // survives" is verifiable. Each id is a zero-padded index in hex.
        let id_for = |i: usize| -> String { format!("{i:064x}") };

        for i in 0..n {
            // Two distinct value spaces so event-ids and pubkeys are disjoint.
            u.note_event(&id_for(i), |_| false);
            u.note_pubkey(&id_for(i + n), |_| false);
        }

        let (events, pubkeys) = u.drain();

        // Each set is independently capped.
        assert_eq!(
            events.len(),
            MAX_UNKNOWN_IDS,
            "event-id set must be hard-capped at MAX_UNKNOWN_IDS under flood"
        );
        assert_eq!(
            pubkeys.len(),
            MAX_UNKNOWN_IDS,
            "pubkey set must be hard-capped at MAX_UNKNOWN_IDS under flood"
        );

        // Oldest-first retention: the FIRST MAX_UNKNOWN_IDS ids inserted are the
        // ones that survive; the surplus (inserted after the cap was hit) is
        // dropped. drain() returns BTreeSet order == insertion order here.
        let expected_events: Vec<String> = (0..MAX_UNKNOWN_IDS).map(id_for).collect();
        assert_eq!(
            events, expected_events,
            "the first-inserted event ids must survive, surplus dropped"
        );
        let expected_pubkeys: Vec<String> =
            (0..MAX_UNKNOWN_IDS).map(|i| id_for(i + n)).collect();
        assert_eq!(
            pubkeys, expected_pubkeys,
            "the first-inserted pubkeys must survive, surplus dropped"
        );
    }

    /// At capacity, a re-reference to an id the set already holds is still
    /// accepted (it does not count against the cap a second time) — so an
    /// already-pending id is never spuriously rejected by the flood guard.
    #[test]
    fn at_capacity_existing_ids_still_accepted() {
        let mut u = UnknownIds::new();
        let id_for = |i: usize| -> String { format!("{i:064x}") };
        for i in 0..MAX_UNKNOWN_IDS {
            u.note_event(&id_for(i), |_| false);
        }
        // Set is now full. Re-referencing an existing id is a no-op success.
        u.note_event(&id_for(0), |_| false);
        // A brand-new id is dropped.
        u.note_event(&id_for(MAX_UNKNOWN_IDS + 1), |_| false);

        let (events, _) = u.drain();
        assert_eq!(events.len(), MAX_UNKNOWN_IDS);
        assert!(
            events.contains(&id_for(0)),
            "an already-pending id stays pending even at capacity"
        );
        assert!(
            !events.contains(&id_for(MAX_UNKNOWN_IDS + 1)),
            "a new id is dropped once the set is at capacity"
        );
    }

    /// `put_back_*` re-insertion also honours the cap — it routes through the
    /// same gate as the collect path, so a put-back can never breach the bound.
    #[test]
    fn put_back_respects_cap() {
        let mut u = UnknownIds::new();
        let id_for = |i: usize| -> String { format!("{i:064x}") };
        // Fill via put_back directly with more than the cap.
        let ids: Vec<String> = (0..MAX_UNKNOWN_IDS + 25).map(id_for).collect();
        u.put_back_events(ids);
        assert_eq!(u.pending_len(), MAX_UNKNOWN_IDS);
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
