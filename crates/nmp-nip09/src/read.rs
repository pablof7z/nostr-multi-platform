//! Generic NIP-09 deletion read seam — decode a kind:5 event's full grammar.
//!
//! A kind:5 event names targets to retract. NIP-09 allows two target shapes and
//! this module decodes both into one canonical [`DeleteRecord`]:
//!
//! * `e` tags — a specific **event id**. Removes that event-id-keyed row.
//! * `a` tags — an **address coordinate** `kind:pubkey:d` (see
//!   [`crate::AddressCoordinate`]). Removes the coordinate-keyed row.
//! * `k` tags — the kind(s) being deleted (informational per NIP-09 §3).
//!
//! Any projection that processes kind:5 deletion events (e.g. `nmp-nip25`'s
//! `ReactionProjection::ingest_delete`, `nmp-nip18`'s repost-wrapper fold,
//! `nmp-content`'s long-form article fold) should call
//! [`DeleteRecord::try_from_kernel_event`] rather than hand-parsing tags.
//! This keeps tag-grammar interpretation centralised in `nmp-nip09` (ADR-0074).
//!
//! Author validation is the consumer's job, not this decoder's: a kind:5 only
//! deletes a target the *same author* published. The decoder exposes the kind:5
//! author ([`DeleteRecord::author`]) and timestamp ([`DeleteRecord::created_at`])
//! so feed adapters can fail closed on a foreign delete and compare against a
//! stored version's `created_at` for coordinate targets. A target that does not
//! parse (an `a` tag that is not an addressable coordinate, an empty `e` tag) is
//! dropped — the delete never guesses an identity it cannot prove.

use nmp_core::substrate::KernelEvent;

use crate::coordinate::AddressCoordinate;

/// Decoded NIP-09 deletion record.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DeleteRecord {
    /// Author of the kind:5 event. A delete is only honoured against targets
    /// this same author published.
    pub author: String,
    /// Timestamp of the kind:5 event. For an addressable (`a`-tag) target, a
    /// deletion only retracts versions created at or before this time — a newer
    /// version published after the deletion request survives (NIP-09 + store
    /// `created_at <= delete.created_at` semantics).
    pub created_at: u64,
    /// Event-id targets (`e` tags). Each removes that event-id-keyed row.
    pub event_targets: Vec<String>,
    /// Address-coordinate targets (`a` tags). Each removes that coordinate row.
    pub address_targets: Vec<AddressCoordinate>,
    /// Kind integers listed in `k` tags (informational per NIP-09 §3; may be
    /// empty). Malformed values that do not parse as `u32` are dropped.
    pub kinds: Vec<u32>,
}

impl DeleteRecord {
    /// Decode a [`KernelEvent`] as a NIP-09 deletion.
    ///
    /// Returns `None` for every non-kind-5 event. Unparseable targets are
    /// silently dropped (fail closed) rather than fabricated. A kind:5 with no
    /// resolvable target yields an empty record — applying it is a no-op.
    #[must_use]
    pub fn try_from_kernel_event(event: &KernelEvent) -> Option<Self> {
        if event.kind != crate::KIND_DELETION {
            return None;
        }
        let mut event_targets = Vec::new();
        let mut address_targets = Vec::new();
        let mut kinds = Vec::new();
        for tag in &event.tags {
            match tag.first().map(String::as_str) {
                Some("e") => {
                    if let Some(id) = tag.get(1).filter(|id| !id.is_empty()) {
                        event_targets.push(id.clone());
                    }
                }
                Some("a") => {
                    if let Some(coord) = tag.get(1).and_then(|raw| AddressCoordinate::parse(raw)) {
                        address_targets.push(coord);
                    }
                }
                Some("k") => {
                    if let Some(k) = tag.get(1).and_then(|raw| raw.parse::<u32>().ok()) {
                        kinds.push(k);
                    }
                }
                _ => {}
            }
        }
        Some(Self {
            author: event.author.clone(),
            created_at: event.created_at,
            event_targets,
            address_targets,
            kinds,
        })
    }

    /// Whether this delete names no resolvable target — applying it is a no-op.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.event_targets.is_empty() && self.address_targets.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn delete(author: &str, tags: Vec<Vec<&str>>) -> KernelEvent {
        KernelEvent {
            id: "del".to_string(),
            author: author.to_string(),
            kind: crate::KIND_DELETION,
            created_at: 9,
            tags: tags
                .into_iter()
                .map(|t| t.into_iter().map(str::to_string).collect())
                .collect(),
            content: String::new(),
            relay_provenance: Vec::new(),
        }
    }

    #[test]
    fn rejects_non_delete_kind() {
        let mut ev = delete("alice", vec![vec!["e", "target"]]);
        ev.kind = 1;
        assert!(DeleteRecord::try_from_kernel_event(&ev).is_none());
    }

    #[test]
    fn decodes_event_id_target() {
        let record =
            DeleteRecord::try_from_kernel_event(&delete("alice", vec![vec!["e", "target"]]))
                .unwrap();
        assert_eq!(record.author, "alice");
        assert_eq!(record.event_targets, vec!["target".to_string()]);
        assert!(record.address_targets.is_empty());
        assert!(!record.is_empty());
    }

    #[test]
    fn decodes_address_coordinate_target() {
        let record = DeleteRecord::try_from_kernel_event(&delete(
            "alice",
            vec![vec!["a", "30023:alice:my-article"]],
        ))
        .unwrap();
        assert_eq!(
            record.address_targets,
            vec![AddressCoordinate::new(30_023, "alice", "my-article")]
        );
        assert!(record.event_targets.is_empty());
    }

    #[test]
    fn decodes_k_tags() {
        let record = DeleteRecord::try_from_kernel_event(&delete(
            "alice",
            vec![vec!["e", "target"], vec!["k", "7"], vec!["k", "1"]],
        ))
        .unwrap();
        assert_eq!(record.kinds, vec![7u32, 1u32]);
    }

    #[test]
    fn drops_malformed_k_values() {
        let record = DeleteRecord::try_from_kernel_event(&delete(
            "alice",
            vec![vec!["e", "target"], vec!["k", "notanumber"], vec!["k", "7"]],
        ))
        .unwrap();
        assert_eq!(record.kinds, vec![7u32]);
    }

    #[test]
    fn ignores_unknown_tags() {
        let record = DeleteRecord::try_from_kernel_event(&delete(
            "alice",
            vec![
                vec!["e", "target"],
                vec!["p", "aaaa"],
                vec!["alt", "something"],
                vec!["h", "group-id"],
            ],
        ))
        .unwrap();
        assert_eq!(record.event_targets, vec!["target".to_string()]);
        assert!(record.kinds.is_empty());
    }

    #[test]
    fn drops_unparseable_targets_fail_closed() {
        // Empty `e` and a non-addressable `a` (kind 1 has no coordinate) are
        // both dropped — the delete never fabricates an identity.
        let record = DeleteRecord::try_from_kernel_event(&delete(
            "alice",
            vec![vec!["e", ""], vec!["a", "1:alice:d"], vec!["a", "garbage"]],
        ))
        .unwrap();
        assert!(
            record.is_empty(),
            "unresolvable targets must not be honoured"
        );
    }

    #[test]
    fn delete_with_no_targets_is_empty_noop() {
        let record = DeleteRecord::try_from_kernel_event(&delete("alice", vec![])).unwrap();
        assert!(record.is_empty());
        assert!(record.kinds.is_empty());
    }
}
