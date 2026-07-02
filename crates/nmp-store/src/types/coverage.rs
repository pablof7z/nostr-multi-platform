//! K3 Stage D1 — coverage-ledger row type (ADR-0072 §3).
//!
//! A `CoverageRow` records, per `(filter_hash, relay)`, the timestamp through
//! which a sync has actually **completed** — EOSE for a plain REQ, NEG-DONE for
//! a NIP-77 negentropy reconciliation. This is the *coverage* the read path
//! needs ("through what timestamp have I COMPLETED a sync for this shape?"), as
//! opposed to the *presence* watermark the live since-floor currently derives
//! from stored events ("what is the newest thing I have that looks like this?").
//! Presence ≠ coverage; the gap between them is the class of permanent backfill
//! holes ADR-0072 §1 names (H1 cross-shape floor poisoning, H2 NEG inherits the
//! floor).
//!
//! ## Honest, downward-closed semantics
//!
//! `covered_through` is the upper bound of a **downward-closed** interval: a row
//! with `covered_through = T` asserts coverage of `[0, T]` and nothing weaker.
//! It is therefore recorded ONLY when the completed sync genuinely covered the
//! window from the origin:
//!
//! - a NEG-DONE reconciliation runs un-floored over `[0, ∞)` (ADR-0072 Stage A),
//!   so it honestly advances `covered_through` to `now`;
//! - an EOSE on an **un-floored** plain REQ (`since` absent or `0`) likewise
//!   covers `[0, now]`;
//! - an EOSE on a **`since`-floored** plain REQ proves coverage only of
//!   `[floor, now]`, NOT `[0, now]`. Advancing `covered_through` to `now` there
//!   would over-claim `[0, floor)` — the very over-claim ADR-0072 §1 says makes
//!   presence unsound. Such an EOSE therefore records NOTHING; the presence
//!   floor remains the floor source for that shape until Stage D2's read swap,
//!   and the un-floored NEG path (Stage A) is what fills the ledger for the
//!   high-value follow-feed shapes.
//!
//! Because the field is downward-closed, `record_coverage` advances it
//! monotonically (`max`): a later completion can only raise the proven bound,
//! never lower it. (The one exception — eviction lowering `covered_through` when
//! it deletes events below the bound — is Stage D3, deliberately out of D1.)

/// A single coverage-ledger entry: through what timestamp a sync has completed
/// for one `(filter_hash, relay)` pair.
///
/// `filter_hash` is the canonical content-address of the subscription shape —
/// the SAME 16-hex-char FNV-1a digest the planner stamps as
/// `SubShape::canonical_filter_hash` and embeds in the wire sub-id
/// (`sub-<filter_hash>`). Keying the ledger by that exact value is what lets the
/// Stage D2 read swap look a row up by the same key `recompile` already builds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CoverageRow {
    /// Canonical filter hash (16 lowercase hex chars) — `canonical_filter_hash`.
    pub filter_hash: String,
    /// Relay URL the sync completed against.
    pub relay: String,
    /// Unix-seconds upper bound of the downward-closed covered interval `[0, T]`.
    pub covered_through: u64,
}

/// Unit-separator byte joining `filter_hash` and `relay` in the ledger key.
///
/// `0x1F` (ASCII Unit Separator) cannot appear in either segment:
/// `filter_hash` is 16 chars drawn from `[0-9a-f]`, and relay URLs are valid
/// UTF-8 `ws`/`wss` strings that never contain a control byte. The key is
/// therefore unambiguously decodable back into `(filter_hash, relay)`.
pub const COVERAGE_KEY_SEP: u8 = 0x1F;

/// Encode `(filter_hash, relay)` into the ledger key bytes.
#[must_use]
pub fn coverage_key(filter_hash: &str, relay: &str) -> Vec<u8> {
    let mut k = Vec::with_capacity(filter_hash.len() + 1 + relay.len());
    k.extend_from_slice(filter_hash.as_bytes());
    k.push(COVERAGE_KEY_SEP);
    k.extend_from_slice(relay.as_bytes());
    k
}

/// Decode a ledger key back into `(filter_hash, relay)`, or `None` if malformed.
#[must_use]
pub fn coverage_key_parts(key: &[u8]) -> Option<(String, String)> {
    let sep = key.iter().position(|&b| b == COVERAGE_KEY_SEP)?;
    let filter_hash = std::str::from_utf8(&key[..sep]).ok()?.to_string();
    let relay = std::str::from_utf8(&key[sep + 1..]).ok()?.to_string();
    Some((filter_hash, relay))
}

/// Predicate type the [`CoverageGuard`] carries: does an event (by its store
/// fields) match the covered shape? Owned by the kernel (the shape-match
/// predicate lives in `nmp-planner`/`nmp-core`, D0), passed opaquely into the
/// store so the store never links protocol/shape logic.
///
/// Args: `(event_id_hex, author_hex, kind, created_at, tags)` — exactly the
/// fields `InterestShape::matches_event_with_id` consumes.
pub type CoverageMatchFn =
    std::sync::Arc<dyn Fn(&str, &str, u32, u64, &[Vec<String>]) -> bool + Send + Sync>;

/// K3 Stage D3 (ADR-0072 §3.D3) — the eviction⇄ledger coherence BACKSTOP input.
///
/// One guard per active covered `(filter_hash, relay)`: it pairs the coverage
/// bound with a predicate that decides whether an about-to-be-evicted event
/// belongs to the covered shape. When LRU eviction deletes an event the guard
/// `matches` whose `created_at <= covered_through`, the store MUST lower that
/// row's `covered_through` to just below the oldest evicted covered event in
/// the **same transaction** as the delete — so the ledger never claims coverage
/// of a range it no longer holds (the permanent-hole class the memory review
/// flagged).
///
/// The kernel derives the guard set from the live coverage rows + active
/// interest registry on each GC pass (relay-agnostic event store, per-relay
/// ledger). When there are no covered rows the guard set is empty and the
/// eviction path is byte-identical to the pin-only path.
#[derive(Clone)]
pub struct CoverageGuard {
    /// Canonical filter hash half of the ledger key.
    pub filter_hash: String,
    /// Relay half of the ledger key.
    pub relay: String,
    /// The downward-closed coverage bound this guard protects.
    pub covered_through: u64,
    /// Does an event (by store fields) match the covered shape? (kernel-owned).
    pub matches: CoverageMatchFn,
}

impl std::fmt::Debug for CoverageGuard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CoverageGuard")
            .field("filter_hash", &self.filter_hash)
            .field("relay", &self.relay)
            .field("covered_through", &self.covered_through)
            .field("matches", &"<fn>")
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_roundtrips_filter_hash_and_relay() {
        let k = coverage_key("deadbeefdeadbeef", "wss://relay.example");
        let (fh, relay) = coverage_key_parts(&k).expect("parts");
        assert_eq!(fh, "deadbeefdeadbeef");
        assert_eq!(relay, "wss://relay.example");
    }

    #[test]
    fn malformed_key_without_separator_is_none() {
        assert!(coverage_key_parts(b"no-separator-here").is_none());
    }
}
