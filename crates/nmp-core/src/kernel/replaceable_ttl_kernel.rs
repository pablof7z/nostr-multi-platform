//! F-TTL replaceable-event freshness + re-verification queue.
//!
//! Extracted from `kernel/mod.rs` (`impl Kernel`) to honour the 500-LOC ceiling.

use super::*;

impl Kernel {
    /// Set the TTL policy for replaceable events (F-TTL).
    pub fn set_replaceable_ttl(&mut self, config: replaceable_ttl::ReplaceableTtlConfig) {
        self.replaceable_ttl = config;
    }

    /// F-TTL — enqueue a replaceable event for re-verification if its freshness has expired.
    pub(crate) fn claim_replaceable(
        &mut self,
        kind: u32,
        pubkey: [u8; 32],
        d_tag: Option<String>,
        force: bool,
    ) {
        // `is_addressable` is the NIP-01 addressable predicate
        // (30000..=39999) — only those identities carry a `d`-tag.
        let key = if crate::store::is_addressable(kind) {
            crate::store::ReplaceableKey::Parameterized {
                kind,
                pubkey,
                d_tag: d_tag.unwrap_or_default(),
            }
        } else {
            crate::store::ReplaceableKey::Regular { kind, pubkey }
        };

        let now = self.now_ms();
        // `force` zeroes the freshness stamp for the gate check below, so a
        // user-initiated refresh always reads as due (`now > 0`) and enqueues
        // a re-fetch even when the cached identity is still within its TTL.
        // No redundant store write: the enqueue path overwrites with
        // `now + INFLIGHT_GUARD_MS` anyway.
        let check_at = if force {
            0
        } else {
            self.store.get_check_again_after(&key).unwrap_or(0)
        };

        // Gate: still fresh, or already in flight → nothing to do.
        if now > check_at && !self.pending_reverify.contains(&key) {
            self.pending_reverify.push_back(key.clone());
            // In-flight guard: prevent re-enqueue until EOSE re-stamps with the
            // real per-kind TTL (or the guard window elapses on a lost EOSE).
            self.store
                .set_check_again_after(key, now + INFLIGHT_GUARD_MS);
        }
    }

    /// Test-only: number of replaceable identities currently queued for re-verification.
    #[cfg(test)]
    pub(crate) fn pending_reverify_len(&self) -> usize {
        self.pending_reverify.len()
    }

    /// Test-only: sub-ids currently tracked for reverify EOSE handling.
    #[cfg(test)]
    pub(crate) fn reverify_sub_ids_for_test(&self) -> Vec<String> {
        self.reverify_subs.keys().cloned().collect()
    }

    /// Test-only: seed a reverify sub_id → key mapping directly.
    #[cfg(test)]
    pub(crate) fn seed_reverify_sub_for_test(
        &mut self,
        sub_id: &str,
        keys: Vec<crate::store::ReplaceableKey>,
    ) {
        self.reverify_subs.insert(sub_id.to_string(), keys);
    }
}
