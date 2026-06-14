//! Wave C identity cluster slice of
//! [`Kernel::builtin_typed_projections`].
//!
//! V-112 (ADR-0042): `author_view` / `thread_view` conditional typed projections
//! deleted. The three built-ins here (`accounts` / `active_account` / `profile`)
//! are all unconditional. Each Model is built from the SAME accessor the generic
//! JSON projection reads, in the same tick, so the typed and JSON wire forms cannot
//! diverge.

use super::{
    encode_accounts, encode_active_account, encode_profile, AccountSummaryRow, AccountsModel,
    ActiveAccountModel, ProfileCardModel, ACCOUNTS_FILE_IDENTIFIER, ACCOUNTS_SCHEMA_ID,
    ACCOUNTS_SCHEMA_VERSION, ACTIVE_ACCOUNT_FILE_IDENTIFIER, ACTIVE_ACCOUNT_SCHEMA_ID,
    ACTIVE_ACCOUNT_SCHEMA_VERSION, PROFILE_FILE_IDENTIFIER, PROFILE_SCHEMA_ID,
    PROFILE_SCHEMA_VERSION,
};
use crate::update_envelope::TypedProjectionData;

/// Map one kernel `ProfileCard` DTO onto the shared [`ProfileCardModel`]. The DTO
/// type is `pub(super)` in `kernel::types`, bound by inference (never named here).
fn profile_card_model(card: &super::super::ProfileCard) -> ProfileCardModel {
    ProfileCardModel {
        pubkey: card.pubkey.clone(),
        // ADR-0032 / V-115: npub field removed from ProfileCard; bech32 is
        // now shell-side. Pass empty string; the FlatBuffers slot is deprecated.
        npub: String::new(),
        display_name: card.display_name.clone(),
        picture_url: card.picture_url.clone(),
        nip05: card.nip05.clone(),
        about: card.about.clone(),
        lnurl: card.lnurl.clone(),
    }
}

impl super::super::Kernel {
    /// Encode the Wave C identity cluster (Tier-2) built-ins as typed
    /// FlatBuffer sidecar entries, in `accounts` → `active_account` → `profile`
    /// order. All three entries are unconditional. Called by
    /// [`builtin_typed_projections`](super::super::Kernel::builtin_typed_projections);
    /// see that method's doc for the mechanism.
    pub(in crate::kernel) fn views_cluster_typed_projections(&self) -> Vec<TypedProjectionData> {
        let mut out = Vec::with_capacity(3);

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
            // ADR-0055 Rung 2: rev + state stamped by make_update after emit.
            ..Default::default()
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
            // ADR-0055 Rung 2: rev + state stamped by make_update after emit.
            ..Default::default()
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
            // ADR-0055 Rung 2: rev + state stamped by make_update after emit.
            ..Default::default()
        });

        out
    }
}
