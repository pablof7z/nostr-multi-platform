//! NIP-09 deletion draft builder — the construction seam for kind:5 events.
//!
//! Protocol crates that need to issue a deletion (e.g. `nmp-nip25` unreacting)
//! call [`build_deletion_draft`] to get an [`OwnedDeletionDraft`] that carries
//! `nmp-nip09` provenance. They do NOT assemble the kind:5 wire event by hand.
//!
//! Validation rules (per NIP-09 §2):
//! - `event_ids` must be non-empty.
//! - Every entry in `event_ids` must be 64 lowercase hex characters.
//! - Every entry in `kinds` is emitted as a `k` tag.
//! - `reason` becomes the event content (may be empty).

use nmp_signer_iface::UnsignedEvent;

use crate::ownership::DELETION_EVENT_PROVENANCE;

/// A kind:5 deletion draft carrying `nmp-nip09` artifact provenance.
pub type OwnedDeletionDraft = nmp_ownership::OwnedEventDraft<UnsignedEvent>;

/// Input to the NIP-09 deletion builder.
///
/// The caller (e.g. `nmp-nip25` `UnreactModule`) is responsible for supplying
/// validated event ids before calling [`build_deletion_draft`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeletionRequest {
    /// Hex-64 event ids to delete. Must be non-empty; every entry validated.
    pub event_ids: Vec<String>,
    /// Optional `k` tags naming the kinds being deleted.
    pub kinds: Vec<u32>,
    /// Human-readable deletion reason (event content, may be empty).
    pub reason: String,
}

/// Build a bare kind:5 NIP-09 deletion event with no pubkey/created_at/sig.
///
/// Returns an [`UnsignedEvent`] carrying the `e`/`k` tags and content. The
/// caller is responsible for wrapping it with provenance before publishing.
///
/// # Errors
///
/// Returns an error string when `event_ids` is empty or any entry is not
/// 64 lowercase hex characters.
pub fn build_deletion_event(req: &DeletionRequest) -> Result<UnsignedEvent, String> {
    if req.event_ids.is_empty() {
        return Err("deletion requires at least one event id".to_string());
    }
    for id in &req.event_ids {
        if !is_hex64(id) {
            return Err(format!(
                "deletion event_id must be 64-hex, got {:?}",
                id
            ));
        }
    }
    let mut tags: Vec<Vec<String>> = Vec::new();
    for id in &req.event_ids {
        tags.push(vec!["e".to_string(), id.clone()]);
    }
    for kind in &req.kinds {
        tags.push(vec!["k".to_string(), kind.to_string()]);
    }
    Ok(UnsignedEvent {
        pubkey: String::new(),
        kind: crate::KIND_DELETION,
        tags,
        content: req.reason.clone(),
        created_at: 0,
    })
}

/// Build an owner-certified kind:5 deletion draft carrying `nmp-nip09` provenance.
///
/// This is the **canonical seam** that every crate must call when it needs to
/// publish a kind:5 deletion event. The returned draft carries
/// [`DELETION_EVENT_PROVENANCE`] so the publish gate accepts it.
///
/// # Errors
///
/// Propagates any error from [`build_deletion_event`].
pub fn build_deletion_draft(req: &DeletionRequest) -> Result<OwnedDeletionDraft, String> {
    let event = build_deletion_event(req)?;
    Ok(OwnedDeletionDraft::new(event, DELETION_EVENT_PROVENANCE))
}

/// Check whether `value` is exactly 64 lowercase hex characters.
pub(crate) fn is_hex64(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const ID_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    #[test]
    fn build_deletion_event_emits_e_tags_and_content() {
        let req = DeletionRequest {
            event_ids: vec![ID_A.to_string(), ID_B.to_string()],
            kinds: vec![],
            reason: "gone".to_string(),
        };
        let event = build_deletion_event(&req).expect("valid request");
        assert_eq!(event.kind, 5);
        assert_eq!(event.content, "gone");
        assert_eq!(event.pubkey, "");
        assert_eq!(event.created_at, 0);
        assert_eq!(event.tags.len(), 2);
        assert_eq!(event.tags[0], vec!["e".to_string(), ID_A.to_string()]);
        assert_eq!(event.tags[1], vec!["e".to_string(), ID_B.to_string()]);
    }

    #[test]
    fn build_deletion_event_emits_k_tags_after_e_tags() {
        let req = DeletionRequest {
            event_ids: vec![ID_A.to_string()],
            kinds: vec![7, 1],
            reason: String::new(),
        };
        let event = build_deletion_event(&req).expect("valid request");
        assert_eq!(event.tags.len(), 3);
        assert_eq!(event.tags[0], vec!["e".to_string(), ID_A.to_string()]);
        assert_eq!(event.tags[1], vec!["k".to_string(), "7".to_string()]);
        assert_eq!(event.tags[2], vec!["k".to_string(), "1".to_string()]);
    }

    #[test]
    fn build_deletion_event_rejects_empty_ids() {
        let req = DeletionRequest {
            event_ids: vec![],
            kinds: vec![],
            reason: String::new(),
        };
        assert!(build_deletion_event(&req).is_err());
    }

    #[test]
    fn build_deletion_event_rejects_non_hex_ids() {
        let req = DeletionRequest {
            event_ids: vec!["not-a-valid-id".to_string()],
            kinds: vec![],
            reason: String::new(),
        };
        let err = build_deletion_event(&req).expect_err("must reject");
        assert!(err.contains("64-hex"), "error: {err}");
    }

    #[test]
    fn build_deletion_event_rejects_short_hex_ids() {
        let req = DeletionRequest {
            event_ids: vec!["aabbcc".to_string()],
            kinds: vec![],
            reason: String::new(),
        };
        assert!(build_deletion_event(&req).is_err());
    }

    #[test]
    fn build_deletion_draft_carries_nip09_provenance() {
        let req = DeletionRequest {
            event_ids: vec![ID_A.to_string()],
            kinds: vec![],
            reason: String::new(),
        };
        let draft = build_deletion_draft(&req).expect("valid request");
        assert_eq!(
            draft.ownership(),
            crate::ownership::DELETION_EVENT_PROVENANCE
        );
        let artifact = draft.ownership().artifact.expect("artifact provenance set");
        assert_eq!(artifact.owner_id, "nmp.nip09");
        assert_eq!(artifact.claim_id, "nostr.kind.5.deletion");
    }

    #[test]
    fn is_hex64_accepts_valid_ids() {
        assert!(is_hex64(ID_A));
        assert!(is_hex64(ID_B));
        assert!(is_hex64(&"0123456789abcdef".repeat(4)));
    }

    #[test]
    fn is_hex64_rejects_non_hex_and_wrong_length() {
        assert!(!is_hex64(""));
        assert!(!is_hex64("aabb"));
        assert!(!is_hex64(&"G".repeat(64)));
        assert!(!is_hex64(&"a".repeat(63)));
        assert!(!is_hex64(&"a".repeat(65)));
    }
}
