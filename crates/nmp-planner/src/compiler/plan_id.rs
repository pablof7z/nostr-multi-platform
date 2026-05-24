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
use crate::interest::{
    InterestLifecycle, InterestScope, LogicalInterest, PTagRouting, Pubkey,
};
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
/// 1. Sorted interests: id + shape (wire JSON) + p-tag routing + scope + lifecycle.
/// 2. Mailbox snapshot for ONLY referenced pubkeys (§3.4 stability rule).
///    Relay vectors within each snapshot are sorted before hashing. kind:10050
///    DM relays are included only for pubkeys referenced by NIP-17 DM-routed
///    `#p` interests.
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
        h.feed_bytes(&[interest.shape.p_tag_routing.plan_hash_tag()]);
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
        };
        h.feed_bytes(&[lifecycle_tag]);
    }

    // ── 2. Mailbox snapshot — referenced pubkeys only ─────────────────────────
    let ref_pks = referenced_pubkeys(interests);
    let dm_ref_pks = dm_relay_referenced_pubkeys(interests);
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
        if dm_ref_pks.contains(pk) {
            if let Some(mut dm_sorted) = cache.dm_inbox_relays(pk) {
                h.feed_bytes(b"dm-relays");
                dm_sorted.sort();
                for r in &dm_sorted { h.feed_bytes(r.as_bytes()); }
            }
        }
    }

    // ── 3. Compile context ────────────────────────────────────────────────────
    h.feed_u64(ctx.indexer_set_version);
    h.feed_u64(ctx.user_config_version);

    // ── 4. Lattice version ────────────────────────────────────────────────────
    h.feed_bytes(&[lattice_version]);

    format!("{:016x}", h.finish())
}

fn dm_relay_referenced_pubkeys(interests: &[LogicalInterest]) -> BTreeSet<Pubkey> {
    let mut pks = BTreeSet::new();
    for interest in interests {
        if interest.shape.p_tag_routing != PTagRouting::Nip17DmRelays {
            continue;
        }
        if let Some(p_values) = interest.shape.tags.get("p") {
            pks.extend(p_values.iter().cloned());
        }
    }
    pks
}

// ─── tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::mailbox::{InMemoryMailboxCache, MailboxSnapshot};
    use crate::interest::{
        InterestId, InterestScope, InterestShape, LogicalInterest, NaddrCoord,
    };
    use std::collections::BTreeSet;

    /// 64-hex-ish pubkey placeholders — content does not matter for hashing,
    /// only that they are distinct and stable across the test.
    const PK_AUTHOR: &str = "1111111111111111111111111111111111111111111111111111111111111111";
    const PK_ADDR: &str = "2222222222222222222222222222222222222222222222222222222222222222";
    const PK_PTAG: &str = "3333333333333333333333333333333333333333333333333333333333333333";
    const PK_UNRELATED: &str =
        "9999999999999999999999999999999999999999999999999999999999999999";

    /// Build a `LogicalInterest` that references one author, one address pubkey,
    /// and one `#p` tag pubkey — exercising all three `referenced_pubkeys` sources.
    fn interest_referencing_all_three() -> LogicalInterest {
        let mut shape = InterestShape::default();
        shape.authors.insert(PK_AUTHOR.to_string());
        shape.addresses.insert(NaddrCoord {
            pubkey: PK_ADDR.to_string(),
            kind: 30023,
            d_tag: "post".to_string(),
        });
        let mut p_values = BTreeSet::new();
        p_values.insert(PK_PTAG.to_string());
        shape.tags.insert("p".to_string(), p_values);
        LogicalInterest {
            id: InterestId(7),
            scope: InterestScope::ActiveAccount,
            shape,
            ..LogicalInterest::default()
        }
    }

    fn relay_snapshot(write: &[&str]) -> MailboxSnapshot {
        MailboxSnapshot {
            write_relays: write.iter().map(|s| s.to_string()).collect(),
            ..MailboxSnapshot::default()
        }
    }

    /// `referenced_pubkeys` must collect from authors ∪ addresses ∪ `#p` tags.
    #[test]
    fn referenced_pubkeys_collects_all_three_sources() {
        let interests = vec![interest_referencing_all_three()];
        let refs = referenced_pubkeys(&interests);
        assert!(
            refs.contains(PK_AUTHOR),
            "author pubkey must be a referenced pubkey"
        );
        assert!(
            refs.contains(PK_ADDR),
            "address-coordinate pubkey must be a referenced pubkey"
        );
        assert!(
            refs.contains(PK_PTAG),
            "#p-tag pubkey must be a referenced pubkey"
        );
        assert!(
            !refs.contains(PK_UNRELATED),
            "unreferenced pubkey must NOT appear in the referenced set"
        );
        assert_eq!(refs.len(), 3, "exactly three pubkeys are referenced");
    }

    /// Happy path: identical inputs hash to an identical 16-hex-char plan-id.
    #[test]
    fn compute_plan_id_is_deterministic() {
        let interests = vec![interest_referencing_all_three()];
        let cache = InMemoryMailboxCache::new();
        let ctx = CompileContext::default();

        let first = compute_plan_id(&interests, &cache, &ctx, 1);
        let second = compute_plan_id(&interests, &cache, &ctx, 1);
        assert_eq!(first, second, "same inputs must yield the same plan-id");
        assert_eq!(first.len(), 16, "plan-id is a 16-hex-char FNV-1a digest");
        assert!(
            first.chars().all(|c| c.is_ascii_hexdigit()),
            "plan-id must be lowercase hex"
        );
    }

    /// §3.4 headline invariant: a mailbox entry for a pubkey that is NOT
    /// referenced by any interest MUST NOT change the plan-id.
    #[test]
    fn unreferenced_mailbox_entry_does_not_change_plan_id() {
        let interests = vec![interest_referencing_all_three()];
        let ctx = CompileContext::default();

        let empty_cache = InMemoryMailboxCache::new();
        let baseline = compute_plan_id(&interests, &empty_cache, &ctx, 1);

        let mut cache_with_noise = InMemoryMailboxCache::new();
        cache_with_noise.put(
            PK_UNRELATED.to_string(),
            relay_snapshot(&["wss://noise.example"]),
        );
        let with_noise = compute_plan_id(&interests, &cache_with_noise, &ctx, 1);

        assert_eq!(
            baseline, with_noise,
            "an unrelated kind:10002 arrival must not perturb the plan-id (§3.4)"
        );
    }

    /// Counterpart to the §3.4 test: a mailbox entry for a REFERENCED pubkey
    /// DOES change the plan-id — proving the referenced-pubkey gate works both ways.
    #[test]
    fn referenced_mailbox_entry_changes_plan_id() {
        let interests = vec![interest_referencing_all_three()];
        let ctx = CompileContext::default();

        let empty_cache = InMemoryMailboxCache::new();
        let baseline = compute_plan_id(&interests, &empty_cache, &ctx, 1);

        let mut cache_with_author = InMemoryMailboxCache::new();
        cache_with_author.put(
            PK_AUTHOR.to_string(),
            relay_snapshot(&["wss://relay.example"]),
        );
        let with_author = compute_plan_id(&interests, &cache_with_author, &ctx, 1);

        assert_ne!(
            baseline, with_author,
            "a mailbox snapshot for a referenced author must change the plan-id"
        );
    }

    /// Interest ordering in the input `Vec` must not affect the plan-id —
    /// the function sorts by `InterestId` before hashing.
    #[test]
    fn plan_id_is_independent_of_interest_order() {
        let mut a = interest_referencing_all_three();
        a.id = InterestId(1);
        let mut b = LogicalInterest::default();
        b.id = InterestId(2);
        b.shape.kinds.insert(1);

        let cache = InMemoryMailboxCache::new();
        let ctx = CompileContext::default();

        let forward = compute_plan_id(&[a.clone(), b.clone()], &cache, &ctx, 1);
        let reversed = compute_plan_id(&[b, a], &cache, &ctx, 1);
        assert_eq!(
            forward, reversed,
            "shuffling the interest Vec must not change the plan-id"
        );
    }

    /// Relay ordering inside a mailbox snapshot must not affect the plan-id —
    /// each relay vector is sorted before hashing.
    #[test]
    fn plan_id_is_independent_of_relay_order_in_snapshot() {
        let interests = vec![interest_referencing_all_three()];
        let ctx = CompileContext::default();

        let mut cache_ab = InMemoryMailboxCache::new();
        cache_ab.put(
            PK_AUTHOR.to_string(),
            relay_snapshot(&["wss://a.example", "wss://b.example"]),
        );
        let mut cache_ba = InMemoryMailboxCache::new();
        cache_ba.put(
            PK_AUTHOR.to_string(),
            relay_snapshot(&["wss://b.example", "wss://a.example"]),
        );

        assert_eq!(
            compute_plan_id(&interests, &cache_ab, &ctx, 1),
            compute_plan_id(&interests, &cache_ba, &ctx, 1),
            "relay-vector order inside a snapshot must not change the plan-id"
        );
    }

    /// Bumping either `CompileContext` version counter must change the plan-id —
    /// policy changes invalidate plans even when the interest set is unchanged.
    #[test]
    fn plan_id_is_sensitive_to_compile_context() {
        let interests = vec![interest_referencing_all_three()];
        let cache = InMemoryMailboxCache::new();

        let baseline = compute_plan_id(&interests, &cache, &CompileContext::default(), 1);

        let bumped_indexer = compute_plan_id(
            &interests,
            &cache,
            &CompileContext { indexer_set_version: 1, user_config_version: 0 },
            1,
        );
        assert_ne!(
            baseline, bumped_indexer,
            "bumping indexer_set_version must change the plan-id"
        );

        let bumped_user = compute_plan_id(
            &interests,
            &cache,
            &CompileContext { indexer_set_version: 0, user_config_version: 1 },
            1,
        );
        assert_ne!(
            baseline, bumped_user,
            "bumping user_config_version must change the plan-id"
        );
    }

    /// A different `lattice_version` byte must change the plan-id —
    /// a merge-lattice rule change invalidates previously compiled plans.
    #[test]
    fn plan_id_is_sensitive_to_lattice_version() {
        let interests = vec![interest_referencing_all_three()];
        let cache = InMemoryMailboxCache::new();
        let ctx = CompileContext::default();

        assert_ne!(
            compute_plan_id(&interests, &cache, &ctx, 1),
            compute_plan_id(&interests, &cache, &ctx, 2),
            "a different lattice_version must change the plan-id"
        );
    }
}
