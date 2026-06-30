//! Noun-free e-tag reference-counter seam.
//!
//! The store maintains, per target event, a set of counters bucketed by a
//! CALLER-SUPPLIED opaque [`ReferenceBucketId`]. It never interprets the
//! buckets: which kinds count, which NIP-10 markers pick the target, and what a
//! bucket *means* (reply / reaction / repost / zap) all live in `nmp-relations`
//! (Layer 4, the cross-protocol aggregation owner per
//! `docs/architecture/crate-boundaries.md` §8), compiled into the opaque
//! [`ReferenceClassifyFn`] and injected at composition time via
//! [`crate::EventStore::install_reference_counter_classifier`].
//!
//! This is the exact shape of the FTS seam (`install_search_index_specs` +
//! [`crate::text_search::CompiledIndexSpec`]): a protocol-aware spec is compiled
//! into a type-erased closure and handed to the store, which runs it at ingest
//! over generic, opaque-keyed buckets. D0: no protocol noun, no kind literal,
//! no NIP-10 marker semantics here.

use std::collections::BTreeMap;

/// An opaque, caller-supplied bucket discriminant for a reference counter.
///
/// `discriminant` is the only thing the store keys on (it is the single trailing
/// byte of the LMDB counter key / the second half of the mem key tuple). `label`
/// is carried for diagnostics only and never affects keying. The store assigns
/// no meaning to a bucket; the owning crate (`nmp-relations`) does.
#[derive(Clone, Copy, Debug)]
pub struct ReferenceBucketId {
    discriminant: u8,
    label: &'static str,
}

impl ReferenceBucketId {
    /// Construct a bucket id from an explicit discriminant + diagnostic label.
    /// The caller owns the discriminant namespace.
    #[must_use]
    pub const fn new(discriminant: u8, label: &'static str) -> Self {
        Self { discriminant, label }
    }

    #[must_use]
    pub const fn discriminant(self) -> u8 {
        self.discriminant
    }

    #[must_use]
    pub const fn label(self) -> &'static str {
        self.label
    }
}

impl PartialEq for ReferenceBucketId {
    fn eq(&self, other: &Self) -> bool {
        self.discriminant == other.discriminant
    }
}
impl Eq for ReferenceBucketId {}

/// The reference-classifier signature: given an event's `kind` and `tags`,
/// return the single `(bucket, target_event_id_hex)` reference edge to count, or
/// `None` if the event is not a counted reference.
///
/// Opaque to the store — produced by `nmp-relations`, type-erased here so the
/// store never names the protocol concept. Mirrors
/// [`crate::text_search::ExtractFn`].
pub type ReferenceClassifyFn =
    dyn Fn(u32, &[Vec<String>]) -> Option<(ReferenceBucketId, String)> + Send + Sync;

/// Aggregated reference counts for one target event, keyed by opaque bucket
/// discriminant.
///
/// Noun-free: the caller maps discriminants back to protocol meaning via the
/// [`ReferenceBucketId`]s it defined. A zero-valued bucket is never stored, so
/// [`Self::is_empty`] is true exactly when the target has no counted references.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TargetReferenceCounts {
    counts: BTreeMap<u8, u64>,
}

impl TargetReferenceCounts {
    /// Record a bucket's count. A zero count is dropped (no zero rows).
    pub fn set(&mut self, bucket: u8, count: u64) {
        if count == 0 {
            self.counts.remove(&bucket);
        } else {
            self.counts.insert(bucket, count);
        }
    }

    /// The count for `bucket`, or 0 if the target has no references in it.
    #[must_use]
    pub fn get(&self, bucket: ReferenceBucketId) -> u64 {
        self.counts.get(&bucket.discriminant()).copied().unwrap_or(0)
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.counts.is_empty()
    }

    /// Iterate `(bucket_discriminant, count)` pairs (ascending discriminant).
    pub fn iter(&self) -> impl Iterator<Item = (u8, u64)> + '_ {
        self.counts.iter().map(|(&k, &v)| (k, v))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buckets_equal_on_discriminant_only() {
        let a = ReferenceBucketId::new(7, "alpha");
        let b = ReferenceBucketId::new(7, "beta");
        assert_eq!(a, b, "equality keys on the discriminant, not the label");
        assert_ne!(a, ReferenceBucketId::new(8, "alpha"));
    }

    #[test]
    fn counts_drop_zero_rows() {
        let mut c = TargetReferenceCounts::default();
        c.set(1, 3);
        c.set(2, 0);
        assert_eq!(c.get(ReferenceBucketId::new(1, "x")), 3);
        assert_eq!(c.get(ReferenceBucketId::new(2, "y")), 0);
        assert!(!c.is_empty());
        c.set(1, 0);
        assert!(c.is_empty());
    }
}
