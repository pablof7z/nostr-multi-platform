//! Typed FlatBuffers wire codec for the kernel-owned `"accounts"` projection
//! (Tier-2 built-in).
//!
//! The authoritative FFI shape is the serde JSON the
//! `snapshot_projections_with_publish_cluster` helper inserts under
//! `"accounts"`: the serialisation of `accounts_enriched()`, a
//! `Vec<AccountSummary>` patched with kind:0 `display_name` / `picture_url`.
//! This module adds a **typed FlatBuffers** encoding of the same shape, carried
//! in the `typed_projections` sidecar (ADR-0037) ALONGSIDE — never replacing —
//! the generic `Value` projection.
//!
//! [`AccountsModel`] is built from the same `accounts_enriched()` vector the
//! JSON path serialises, in the same tick, so the two wire forms cannot
//! structurally diverge.
//!
//! `AccountSummary.picture_url` carries serde `skip_serializing_if`, so its JSON
//! key is OMITTED when `None`; `display_name` has no skip, so it serialises as
//! `null`-when-`None`. The typed buffer carries both presences as explicit
//! `has_*` flags.
//!
//! Honours D6 (no panics): decode returns `Err(String)` on any malformed input.

// The generated FlatBuffers bindings are intrinsically `unsafe`. This `allow`
// block scopes the relaxation to the single generated module.
#[allow(
    clippy::all,
    dead_code,
    deprecated,
    missing_docs,
    non_camel_case_types,
    non_snake_case,
    unsafe_code,
    unused_imports
)]
#[path = "generated/accounts_generated.rs"]
pub mod generated;

use flatbuffers::{FlatBufferBuilder, WIPOffset};

use generated::nmp::kernel as fb;

/// Stable schema identifier carried in the typed-projection envelope. Equals the
/// snapshot key (ADR-0037 shared-keyspace contract).
pub const ACCOUNTS_SCHEMA_ID: &str = "accounts";
/// FlatBuffers file identifier embedded in every buffer this module emits.
pub const ACCOUNTS_FILE_IDENTIFIER: &[u8; 4] = b"KACC";
/// Wire schema version. Bump on any breaking change to `accounts.fbs`.
pub const ACCOUNTS_SCHEMA_VERSION: u32 = 1;

/// One account row — a field-for-field mirror of one
/// [`AccountSummary`](crate::kernel::AccountSummary). `Option<String>` fields
/// are flattened to `Option<String>` here and encoded as `has_x` + value.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AccountSummaryRow {
    pub id: String,
    pub npub: String,
    pub display_name: Option<String>,
    pub signer_kind: String,
    pub status: String,
    pub signer_label: String,
    pub signer_is_remote: bool,
    pub is_active: bool,
    pub picture_url: Option<String>,
}

/// The `"accounts"` read model — the ordered account rows. Built from the same
/// `accounts_enriched()` vector the JSON projection serialises.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AccountsModel {
    pub accounts: Vec<AccountSummaryRow>,
}

// --- encode ---------------------------------------------------------------

/// Encode an [`AccountsModel`] to typed FlatBuffers bytes (with the `KACC` file
/// identifier). Row order is preserved verbatim.
#[must_use]
pub(crate) fn encode_accounts(model: &AccountsModel) -> Vec<u8> {
    let mut fbb = FlatBufferBuilder::new();

    let row_offsets: Vec<WIPOffset<fb::AccountSummaryRow>> = model
        .accounts
        .iter()
        .map(|row| {
            let id = fbb.create_string(&row.id);
            let npub = fbb.create_string(&row.npub);
            let display_name = row
                .display_name
                .as_ref()
                .map(|value| fbb.create_string(value));
            let signer_kind = fbb.create_string(&row.signer_kind);
            let status = fbb.create_string(&row.status);
            let signer_label = fbb.create_string(&row.signer_label);
            let picture_url = row
                .picture_url
                .as_ref()
                .map(|value| fbb.create_string(value));
            fb::AccountSummaryRow::create(
                &mut fbb,
                &fb::AccountSummaryRowArgs {
                    id: Some(id),
                    npub: Some(npub),
                    has_display_name: row.display_name.is_some(),
                    display_name,
                    signer_kind: Some(signer_kind),
                    status: Some(status),
                    signer_label: Some(signer_label),
                    signer_is_remote: row.signer_is_remote,
                    is_active: row.is_active,
                    has_picture_url: row.picture_url.is_some(),
                    picture_url,
                },
            )
        })
        .collect();
    let accounts = fbb.create_vector(&row_offsets);

    let root = fb::AccountsSnapshot::create(
        &mut fbb,
        &fb::AccountsSnapshotArgs {
            accounts: Some(accounts),
        },
    );
    fb::finish_accounts_snapshot_buffer(&mut fbb, root);
    fbb.finished_data().to_vec()
}

// --- decode ---------------------------------------------------------------

/// Decode typed FlatBuffers bytes (as produced by [`encode_accounts`]) back
/// into an [`AccountsModel`]. Returns an error string on any malformed input.
pub fn decode_accounts(bytes: &[u8]) -> Result<AccountsModel, String> {
    if bytes.len() < 8 || !fb::accounts_snapshot_buffer_has_identifier(bytes) {
        return Err("missing KACC file identifier".to_string());
    }
    let root = fb::root_as_accounts_snapshot(bytes)
        .map_err(|e| format!("not a valid AccountsSnapshot buffer: {e}"))?;

    let mut accounts = Vec::new();
    if let Some(fb_accounts) = root.accounts() {
        accounts.reserve(fb_accounts.len());
        for row in fb_accounts.iter() {
            accounts.push(AccountSummaryRow {
                id: row.id().unwrap_or_default().to_string(),
                npub: row.npub().unwrap_or_default().to_string(),
                display_name: row
                    .has_display_name()
                    .then(|| row.display_name().unwrap_or_default().to_string()),
                signer_kind: row.signer_kind().unwrap_or_default().to_string(),
                status: row.status().unwrap_or_default().to_string(),
                signer_label: row.signer_label().unwrap_or_default().to_string(),
                signer_is_remote: row.signer_is_remote(),
                is_active: row.is_active(),
                picture_url: row
                    .has_picture_url()
                    .then(|| row.picture_url().unwrap_or_default().to_string()),
            });
        }
    }

    Ok(AccountsModel { accounts })
}

#[cfg(test)]
#[path = "accounts_fb_tests.rs"]
mod tests;
