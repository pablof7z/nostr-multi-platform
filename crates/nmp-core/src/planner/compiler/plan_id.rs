//! Plan-id hashing: `CompileContext` and `compute_plan_id`.
//!
//! The plan-id is a content-addressed string that uniquely identifies a
//! compiled plan. It covers only the inputs that actually affect routing:
//! referenced pubkeys (not the full mailbox cache), interest shapes, scopes,
//! and version counters.
//!
//! Design: `docs/design/subscription-compilation/compiler.md` §3.4
//! Doctrine: D8 (plan-id stability avoids redundant recompilation).

use std::collections::BTreeSet;
use crate::planner::interest::{InterestLifecycle, InterestScope, LogicalInterest, Pubkey};
use super::mailbox::MailboxCache;

// ─── CompileContext ───────────────────────────────────────────────────────────

/// Versioning inputs for plan-id binding (§3.4).
///
/// Both counters advance whenever the corresponding policy changes:
/// - `indexer_set_version` — bumped when the kernel's indexer relay set changes.
/// - `user_config_version` — bumped when user-configured relay settings change.
///
/// Including these in the plan-id hash ensures that plan-ids invalidate when
/// policy changes even if the interest set itself is unchanged.
///
/// Design: `docs/design/subscription-compilation/compiler.md` §3.4
#[derive(Clone, Debug, Default)]
pub struct CompileContext {
    /// Monotonic counter advancing on every accepted change to the indexer set.
    pub indexer_set_version: u64,
    /// Monotonic counter advancing on every accepted change to user-configured relays.
    pub user_config_version: u64,
}

// ─── FNV-1a hasher ───────────────────────────────────────────────────────────

/// FNV-1a hasher (64-bit).
///
/// Phase 1 implementation. Phase 2 will upgrade to blake3 when that crate
/// joins the workspace.
struct FnvHasher(u64);

impl FnvHasher {
    fn new() -> Self {
        Self(0xcbf29ce484222325)
    }
    fn feed_bytes(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 ^= u64::from(b);
            self.0 = self.0.wrapping_mul(0x100000001b3);
        }
    }
    fn feed_u64(&mut self, v: u64) {
        self.feed_bytes(&v.to_le_bytes());
    }
    fn finish(self) -> u64 {
        self.0
    }
}

// ─── Referenced pubkeys ───────────────────────────────────────────────────────

/// Collect all pubkeys that are referenced by the interest set.
///
/// Per §3.4: only the mailbox entries for **referenced** pubkeys participate
/// in the plan-id hash. An unrelated kind:10002 arrival (for a pubkey not in
/// any interest's author set, #p tags, or address pubkeys) MUST NOT change
/// the plan-id.
///
/// Referenced pubkeys = `interest.shape.authors ∪ addresses[*].pubkey ∪ tags["p"][*]`
pub(super) fn referenced_pubkeys(interests: &[LogicalInterest]) -> BTreeSet<Pubkey> {
    let mut pks = BTreeSet::new();
    for interest in interests {
        pks.extend(interest.shape.authors.iter().cloned());
        for coord in &interest.shape.addresses {
            pks.insert(coord.pubkey.clone());
        }
        if let Some(p_values) = interest.shape.tags.get("p") {
            pks.extend(p_values.iter().cloned());
        }
    }
    pks
}

// ─── compute_plan_id ─────────────────────────────────────────────────────────

/// Compute a stable, deterministic plan-id string.
///
/// Hash inputs (all sorted for determinism):
/// 1. Sorted interests: id + shape (JSON) + scope + lifecycle.
/// 2. Mailbox snapshot for ONLY referenced pubkeys (§3.4 stability rule).
///    Relay vectors within each snapshot are sorted before hashing.
/// 3. Compile context: `indexer_set_version` + `user_config_version`.
/// 4. Merge lattice version.
///
/// An unrelated kind:10002 arrival (for a pubkey not in any interest's author
/// set / #p tags / address pubkeys) MUST NOT change the plan-id.
pub(super) fn compute_plan_id(
    interests: &[LogicalInterest],
    cache: &dyn MailboxCache,
    ctx: &CompileContext,
    lattice_version: u8,
) -> String {
    let mut h = FnvHasher::new();

    // ── 1. Sorted interest contributions ─────────────────────────────────────
    let mut sorted_interests: Vec<&LogicalInterest> = interests.iter().collect();
    sorted_interests.sort_by_key(|i| &i.id);
    for interest in sorted_interests {
        h.feed_u64(interest.id.0);
        if let Ok(shape_json) = serde_json::to_vec(&interest.shape) {
            h.feed_bytes(&shape_json);
        }
        let scope_tag: u8 = match &interest.scope {
            InterestScope::ActiveAccount => 0,
            InterestScope::Account(acct) => {
                h.feed_bytes(acct.as_bytes());
                1
            }
            InterestScope::Global => 2,
        };
        h.feed_bytes(&[scope_tag]);
        let lifecycle_tag: u8 = match &interest.lifecycle {
            InterestLifecycle::Tailing => 0,
            InterestLifecycle::OneShot => 1,
            InterestLifecycle::BoundedTime { until_ms } => {
                h.feed_u64(*until_ms);
                2
            }
        };
        h.feed_bytes(&[lifecycle_tag]);
    }

    // ── 2. Mailbox snapshot — referenced pubkeys only ─────────────────────────
    let ref_pks = referenced_pubkeys(interests);
    for pk in &ref_pks {
        if let Some(mb) = cache.get(pk) {
            h.feed_bytes(pk.as_bytes());
            let mut write_sorted = mb.write_relays.clone();
            write_sorted.sort();
            for r in &write_sorted { h.feed_bytes(r.as_bytes()); }
            let mut read_sorted = mb.read_relays.clone();
            read_sorted.sort();
            for r in &read_sorted { h.feed_bytes(r.as_bytes()); }
            let mut both_sorted = mb.both_relays.clone();
            both_sorted.sort();
            for r in &both_sorted { h.feed_bytes(r.as_bytes()); }
        }
    }

    // ── 3. Compile context ────────────────────────────────────────────────────
    h.feed_u64(ctx.indexer_set_version);
    h.feed_u64(ctx.user_config_version);

    // ── 4. Lattice version ────────────────────────────────────────────────────
    h.feed_bytes(&[lattice_version]);

    format!("{:016x}", h.finish())
}
