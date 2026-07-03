use super::command_apply::CommandApplyOutcome;
use crate::publish::PublishTarget;
use nmp_signer_iface::UnsignedEvent;

impl super::KernelReducer {
    pub(super) fn apply_contact_edit(
        &mut self,
        pubkeys: Vec<String>,
        add: bool,
        correlation_id: Option<String>,
    ) -> CommandApplyOutcome {
        use CommandApplyOutcome::{NeedsSign, Unsupported};

        let Some(account_pubkey) = self.active_account_pubkey() else {
            return Unsupported {
                reason: "no active account for contact-list edit".to_string(),
            };
        };
        let Some(mut current) = self.kernel.try_current_contact_list_event_for_edit() else {
            return Unsupported {
                reason: "follow_list_not_loaded".to_string(),
            };
        };

        let is_single_target = pubkeys.len() == 1;
        let mut draft = None;
        for pubkey in pubkeys {
            if !crate::kernel::is_hex_pubkey(&pubkey) {
                if is_single_target {
                    let verb = if add { "follow" } else { "unfollow" };
                    return Unsupported {
                        reason: format!("{verb}: expected 64-hex pubkey"),
                    };
                }
                continue;
            }
            if pubkey == account_pubkey {
                continue;
            }
            let created_at = self.now_secs().max(current.created_at.saturating_add(1));
            let next_draft = if add {
                self.kernel.contact_list_reader().draft_after_add(
                    &account_pubkey,
                    &current,
                    &pubkey,
                    created_at,
                )
            } else {
                self.kernel.contact_list_reader().draft_after_remove(
                    &account_pubkey,
                    &current,
                    &pubkey,
                    created_at,
                )
            };
            let Some(next_draft) = next_draft else {
                return Unsupported {
                    reason: "contact_list_writer_not_installed".to_string(),
                };
            };
            current = crate::slots::ContactListEvent {
                tags: next_draft.tags.clone(),
                content: next_draft.content.clone(),
                created_at: next_draft.created_at,
            };
            draft = Some(next_draft);
        }

        let Some(draft) = draft else {
            return Unsupported {
                reason: "contact-list edit had no valid target".to_string(),
            };
        };
        let unsigned = UnsignedEvent {
            pubkey: draft.pubkey,
            kind: draft.kind,
            tags: draft.tags,
            content: draft.content,
            created_at: draft.created_at,
        };
        let unsigned_json = serde_json::json!({
            "pubkey": unsigned.pubkey.clone(),
            "kind": unsigned.kind,
            "tags": unsigned.tags,
            "content": unsigned.content,
            "created_at": unsigned.created_at,
        })
        .to_string();
        match self.begin_sign_roundtrip_at(
            unsigned.pubkey,
            &unsigned_json,
            crate::time::Instant::now(),
        ) {
            Ok(request) => NeedsSign {
                request,
                target: PublishTarget::Auto,
                action_correlation_id: correlation_id,
            },
            Err(reason) => Unsupported { reason },
        }
    }
}
