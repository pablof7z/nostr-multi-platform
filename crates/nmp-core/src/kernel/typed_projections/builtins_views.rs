//! Wave C identity + views-cluster slice of
//! [`Kernel::builtin_typed_projections`].
//!
//! The five built-ins here (`accounts` / `active_account` / `profile` /
//! `author_view` / `thread_view`) carry heavier nested DTO→Model mappings than
//! the relay/settings built-ins, and the two view built-ins are
//! **conditionally** emitted. Their mappings must be inlined where the
//! `pub(super)`/`pub(crate)` DTO types (`AccountSummary`, `ProfileCard`,
//! `AuthorViewPayload`, `ThreadViewPayload`, `TimelineItem`, `ProfileAction`,
//! ...) are reachable — i.e. in a `kernel::` descendant — but kept under the
//! same owner and out of `mod.rs` so that file stays under the LOC ceiling. Each
//! Model is built from the SAME accessor the generic JSON projection in
//! [`snapshot_projections_with_publish_cluster`](super::super::Kernel::snapshot_projections_with_publish_cluster)
//! reads, in the same tick, so the typed and JSON wire forms cannot diverge:
//!
//! - `accounts`        ← `accounts_enriched()` (NOT the unenriched
//!                        `account_snapshot().0`).
//! - `active_account`  ← `account_snapshot().1` (unconditional; `None` ⇒
//!                        `has_active_account = false`, mirroring JSON `null`).
//! - `profile`         ← `profile_card()`.
//! - `author_view`     ← `author_view()`  — pushed ONLY when `Some` (D5).
//! - `thread_view`     ← `thread_view()`  — pushed ONLY when `Some` (D5).
//!
//! D5: the two view entries are absent from the sidecar exactly when their JSON
//! keys are absent — never an empty placeholder buffer.

use super::{
    encode_accounts, encode_active_account, encode_author_view, encode_profile, encode_thread_view,
    AccountSummaryRow, AccountsModel, ActiveAccountModel, AuthorViewModel, ProfileActionModel,
    ProfileCardModel, ProfileDispatchSpecModel, ThreadViewModel, TimelineItemModel,
    ACCOUNTS_FILE_IDENTIFIER, ACCOUNTS_SCHEMA_ID, ACCOUNTS_SCHEMA_VERSION,
    ACTIVE_ACCOUNT_FILE_IDENTIFIER, ACTIVE_ACCOUNT_SCHEMA_ID, ACTIVE_ACCOUNT_SCHEMA_VERSION,
    AUTHOR_VIEW_FILE_IDENTIFIER, AUTHOR_VIEW_SCHEMA_ID, AUTHOR_VIEW_SCHEMA_VERSION,
    PROFILE_FILE_IDENTIFIER, PROFILE_SCHEMA_ID, PROFILE_SCHEMA_VERSION,
    THREAD_VIEW_FILE_IDENTIFIER, THREAD_VIEW_SCHEMA_ID, THREAD_VIEW_SCHEMA_VERSION,
};
use crate::update_envelope::TypedProjectionData;

/// Map one kernel `ProfileCard` DTO onto the shared [`ProfileCardModel`]. Bound
/// by reference so it works for both the standalone `profile` projection and the
/// `author_view`'s nested `profile`. The DTO type is `pub(super)` in
/// `kernel::types`, so it is bound by inference (never named here).
fn profile_card_model(card: &super::super::ProfileCard) -> ProfileCardModel {
    ProfileCardModel {
        pubkey: card.pubkey.clone(),
        npub: card.npub.clone(),
        display_name: card.display_name.clone(),
        picture_url: card.picture_url.clone(),
        nip05: card.nip05.clone(),
        about: card.about.clone(),
        has_profile: card.has_profile,
        lnurl: card.lnurl.clone(),
    }
}

impl super::super::Kernel {
    /// Encode the Wave C identity + views-cluster (Tier-2) built-ins as typed
    /// FlatBuffer sidecar entries, in `accounts` → `active_account` → `profile`
    /// → `author_view`? → `thread_view`? order. The two view entries are pushed
    /// only when the corresponding view is open (D5). Called by
    /// [`builtin_typed_projections`](super::super::Kernel::builtin_typed_projections);
    /// see that method's doc for the mechanism.
    pub(in crate::kernel) fn views_cluster_typed_projections(&self) -> Vec<TypedProjectionData> {
        let mut out = Vec::with_capacity(5);

        // `accounts` — encoded from the SAME `accounts_enriched()` vector the
        // JSON path serialises (enriched with kind:0 picture_url / display_name;
        // NOT the unenriched `account_snapshot().0`).
        let accounts = AccountsModel {
            accounts: self
                .accounts_enriched()
                .iter()
                .map(|acc| AccountSummaryRow {
                    id: acc.id.clone(),
                    npub: acc.npub.clone(),
                    display_name: acc.display_name.clone(),
                    signer_kind: acc.signer_kind.clone(),
                    status: acc.status.clone(),
                    signer_label: acc.signer_label.clone(),
                    signer_is_remote: acc.signer_is_remote,
                    is_active: acc.is_active,
                    picture_url: acc.picture_url.clone(),
                })
                .collect(),
        };
        out.push(TypedProjectionData {
            key: ACCOUNTS_SCHEMA_ID.to_string(),
            schema_id: ACCOUNTS_SCHEMA_ID.to_string(),
            schema_version: ACCOUNTS_SCHEMA_VERSION,
            file_identifier: String::from_utf8_lossy(ACCOUNTS_FILE_IDENTIFIER).into_owned(),
            payload: encode_accounts(&accounts),
        });

        // `active_account` — encoded from the SAME `account_snapshot().1` the
        // JSON path reads. Unconditional; `None` ⇒ `has_active_account = false`
        // (mirrors JSON `null`).
        let (_, active_account) = self.account_snapshot();
        let active_account = ActiveAccountModel {
            pubkey: active_account.cloned(),
        };
        out.push(TypedProjectionData {
            key: ACTIVE_ACCOUNT_SCHEMA_ID.to_string(),
            schema_id: ACTIVE_ACCOUNT_SCHEMA_ID.to_string(),
            schema_version: ACTIVE_ACCOUNT_SCHEMA_VERSION,
            file_identifier: String::from_utf8_lossy(ACTIVE_ACCOUNT_FILE_IDENTIFIER).into_owned(),
            payload: encode_active_account(&active_account),
        });

        // `profile` — encoded from the SAME `profile_card()` output the JSON
        // path serialises.
        let profile = profile_card_model(&self.profile_card());
        out.push(TypedProjectionData {
            key: PROFILE_SCHEMA_ID.to_string(),
            schema_id: PROFILE_SCHEMA_ID.to_string(),
            schema_version: PROFILE_SCHEMA_VERSION,
            file_identifier: String::from_utf8_lossy(PROFILE_FILE_IDENTIFIER).into_owned(),
            payload: encode_profile(&profile),
        });

        // `author_view` — encoded from the SAME `author_view()` output the JSON
        // path serialises. D5: pushed ONLY when the view is open (the JSON key is
        // likewise OMITTED when `None`).
        if let Some(view) = self.author_view() {
            let model = AuthorViewModel {
                pubkey: view.pubkey.clone(),
                state: view.state.clone(),
                profile: profile_card_model(&view.profile),
                items: view.items.iter().map(timeline_item_model).collect(),
                note_count: view.note_count as u64,
                note_count_display: view.note_count_display.clone(),
                primary_action: view
                    .primary_action
                    .as_ref()
                    .map(|action| ProfileActionModel {
                        kind: action.kind.to_string(),
                        label: action.label.to_string(),
                        target_pubkey: action.target_pubkey.clone(),
                        icon_name: action.icon_name.to_string(),
                        dispatch: action
                            .dispatch
                            .as_ref()
                            .map(|spec| ProfileDispatchSpecModel {
                                namespace: spec.namespace.to_string(),
                                body_json: spec.body_json.clone(),
                            }),
                    }),
            };
            out.push(TypedProjectionData {
                key: AUTHOR_VIEW_SCHEMA_ID.to_string(),
                schema_id: AUTHOR_VIEW_SCHEMA_ID.to_string(),
                schema_version: AUTHOR_VIEW_SCHEMA_VERSION,
                file_identifier: String::from_utf8_lossy(AUTHOR_VIEW_FILE_IDENTIFIER).into_owned(),
                payload: encode_author_view(&model),
            });
        }

        // `thread_view` — encoded from the SAME `thread_view()` output the JSON
        // path serialises. D5: pushed ONLY when the view is open.
        if let Some(view) = self.thread_view() {
            let model = ThreadViewModel {
                focused_event_id: view.focused_event_id.clone(),
                root_event_id: view.root_event_id.clone(),
                state: view.state.clone(),
                items: view.items.iter().map(timeline_item_model).collect(),
                previous_count: view.previous_count as u64,
                next_count: view.next_count as u64,
                previous_count_label: view.previous_count_label.clone(),
                next_count_label: view.next_count_label.clone(),
            };
            out.push(TypedProjectionData {
                key: THREAD_VIEW_SCHEMA_ID.to_string(),
                schema_id: THREAD_VIEW_SCHEMA_ID.to_string(),
                schema_version: THREAD_VIEW_SCHEMA_VERSION,
                file_identifier: String::from_utf8_lossy(THREAD_VIEW_FILE_IDENTIFIER).into_owned(),
                payload: encode_thread_view(&model),
            });
        }

        out
    }
}

/// Map one kernel `TimelineItem` DTO onto the shared [`TimelineItemModel`]. The
/// DTO type is `pub(crate)` in `kernel::types`, bound by inference. Shared by the
/// `author_view` and `thread_view` mappings above.
fn timeline_item_model(item: &super::super::TimelineItem) -> TimelineItemModel {
    TimelineItemModel {
        id: item.id.clone(),
        author_pubkey: item.author_pubkey.clone(),
        author_picture_url: item.author_picture_url.clone(),
        author_lnurl: item.author_lnurl.clone(),
        author_display_name: item.author_display_name.clone(),
        kind: item.kind,
        content: item.content.clone(),
        content_preview: item.content_preview.clone(),
        created_at: item.created_at,
        relay_count: item.relay_count,
        is_repost: item.is_repost,
        nav_target_id: item.nav_target_id.clone(),
        repost_inner_content: item.repost_inner_content.clone(),
    }
}
