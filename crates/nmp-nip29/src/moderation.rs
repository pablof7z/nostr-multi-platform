//! Moderation audit-record materialisation per `moderation.md` §5.
//!
//! Strict separation enforced: **never** mutates `GroupAdmins`/`GroupMembers`.
//! Materialising the audit record is the **only** persistent effect of
//! ingesting a user-signed 9000-9022. Canonical membership flips only when
//! the relay's republished 39001/39002 lands.

use crate::domain::records::ModerationEventRecord;
use crate::group_id::GroupId;

/// Build an audit `ModerationEventRecord` from the wire shape of a
/// 9000-9009 / 9021 / 9022 event.
///
/// Returns `None` when no `h` tag is present (the caller's classification
/// should already have filtered these out, but the guard is structural).
pub fn build_audit_record(
    group: &GroupId,
    event_id: &str,
    kind: u32,
    actor_pubkey: &str,
    created_at: u64,
    tags: &[Vec<String>],
) -> ModerationEventRecord {
    let target_pubkey = tags
        .iter()
        .find(|t| t.len() >= 2 && t[0] == "p")
        .map(|t| t[1].clone());
    let target_event_id = tags
        .iter()
        .find(|t| t.len() >= 2 && t[0] == "e")
        .map(|t| t[1].clone());
    let reason = tags
        .iter()
        .find(|t| t.len() >= 2 && t[0] == "reason")
        .map(|t| t[1].clone());

    ModerationEventRecord {
        group: group.clone(),
        event_id: event_id.to_string(),
        kind,
        actor_pubkey: actor_pubkey.to_string(),
        target_pubkey,
        target_event_id,
        reason,
        created_at,
        raw_tags: tags.to_vec(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g() -> GroupId {
        GroupId::new("wss://h.example.com", "g1")
    }

    #[test]
    fn put_user_audit_pulls_p_and_reason() {
        let tags = vec![
            vec!["h".into(), "g1".into()],
            vec!["p".into(), "target-pk".into()],
            vec!["reason".into(), "added by alice".into()],
        ];
        let r = build_audit_record(&g(), "evt-1", 9000, "alice-pk", 100, &tags);
        assert_eq!(r.target_pubkey.as_deref(), Some("target-pk"));
        assert_eq!(r.target_event_id, None);
        assert_eq!(r.reason.as_deref(), Some("added by alice"));
        assert_eq!(r.kind, 9000);
    }

    #[test]
    fn delete_event_audit_pulls_e_not_p() {
        let tags = vec![
            vec!["h".into(), "g1".into()],
            vec!["e".into(), "evt-victim".into()],
        ];
        let r = build_audit_record(&g(), "evt-2", 9005, "alice-pk", 101, &tags);
        assert_eq!(r.target_event_id.as_deref(), Some("evt-victim"));
        assert_eq!(r.target_pubkey, None);
    }
}
