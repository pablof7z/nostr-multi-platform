//! Typed FlatBuffers wire codec for the actor-owned `"nip46_onboarding"`
//! projection (Tier-1 closure path).
//!
//! The authoritative FFI shape is the serde JSON the
//! `registry.register("nip46_onboarding", …)` closure in
//! `crates/nmp-core/src/actor/mod.rs` inserts under `"nip46_onboarding"`: the
//! serialisation of the `Nip46OnboardingDto` built by
//! `build_nip46_onboarding_dto`. UNLIKE `bunker_handshake`, this projection is
//! always present (never JSON `null`) — the static signer-app probe table is
//! emitted even when no handshake is in flight — so the typed closure always
//! returns `Some`. This module adds a **typed FlatBuffers** encoding of the same
//! shape, carried in the `typed_projections` sidecar (ADR-0037) ALONGSIDE —
//! never replacing — the generic `Value` projection.
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
#[path = "generated/nip46_onboarding_generated.rs"]
pub mod generated;

use flatbuffers::{FlatBufferBuilder, WIPOffset};

use generated::nmp::kernel as fb;

/// Stable schema identifier carried in the typed-projection envelope. Equals the
/// snapshot key (ADR-0037 shared-keyspace contract).
pub const NIP46_ONBOARDING_SCHEMA_ID: &str = "nip46_onboarding";
/// FlatBuffers file identifier embedded in every buffer this module emits.
pub(crate) const NIP46_ONBOARDING_FILE_IDENTIFIER: &[u8; 4] = b"KN46";
/// Wire schema version. Bump on any breaking change to `nip46_onboarding.fbs`.
pub(crate) const NIP46_ONBOARDING_SCHEMA_VERSION: u32 = 1;

/// One row of the static signer-app probe table — a field-for-field mirror of
/// one SERIALISED `SignerAppDescriptor`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SignerAppRow {
    pub scheme: String,
    pub display_label: String,
    pub signer_kind: String,
}

/// A field-for-field mirror of the SERIALISED `Nip46OnboardingDto`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Nip46OnboardingModel {
    pub signer_apps: Vec<SignerAppRow>,
    /// Typed handshake stage as a snake_case wire token; `None` when no
    /// handshake is in flight (mirrors the JSON `null`).
    pub stage_kind: Option<String>,
    pub progress_message: Option<String>,
    pub is_in_flight: bool,
    pub is_failed: bool,
    pub is_terminal_success: bool,
    pub can_cancel: bool,
}

// --- encode ---------------------------------------------------------------

/// Encode one [`SignerAppRow`] into this module's generated `SignerApp` table.
fn create_signer_app<'a>(
    fbb: &mut FlatBufferBuilder<'a>,
    row: &SignerAppRow,
) -> WIPOffset<fb::SignerApp<'a>> {
    let scheme = fbb.create_string(&row.scheme);
    let display_label = fbb.create_string(&row.display_label);
    let signer_kind = fbb.create_string(&row.signer_kind);
    fb::SignerApp::create(
        fbb,
        &fb::SignerAppArgs {
            scheme: Some(scheme),
            display_label: Some(display_label),
            signer_kind: Some(signer_kind),
        },
    )
}

/// Encode a [`Nip46OnboardingModel`] to typed FlatBuffers bytes (with the `KN46`
/// file identifier).
#[must_use]
pub(crate) fn encode_nip46_onboarding(model: &Nip46OnboardingModel) -> Vec<u8> {
    let mut fbb = FlatBufferBuilder::new();
    let app_offsets: Vec<WIPOffset<fb::SignerApp<'_>>> = model
        .signer_apps
        .iter()
        .map(|row| create_signer_app(&mut fbb, row))
        .collect();
    let signer_apps = fbb.create_vector(&app_offsets);
    let stage_kind = model.stage_kind.as_ref().map(|v| fbb.create_string(v));
    let progress_message = model.progress_message.as_ref().map(|v| fbb.create_string(v));
    let root = fb::Nip46Onboarding::create(
        &mut fbb,
        &fb::Nip46OnboardingArgs {
            signer_apps: Some(signer_apps),
            has_stage_kind: model.stage_kind.is_some(),
            stage_kind,
            has_progress_message: model.progress_message.is_some(),
            progress_message,
            is_in_flight: model.is_in_flight,
            is_failed: model.is_failed,
            is_terminal_success: model.is_terminal_success,
            can_cancel: model.can_cancel,
        },
    );
    fb::finish_nip_46_onboarding_buffer(&mut fbb, root);
    fbb.finished_data().to_vec()
}

// --- decode ---------------------------------------------------------------

/// Decode typed FlatBuffers bytes (as produced by [`encode_nip46_onboarding`])
/// back into a [`Nip46OnboardingModel`]. Returns an error string on any
/// malformed input. Available to sibling crates (e.g. desktop shells) for
/// decoding the `"nip46_onboarding"` typed sidecar from a snapshot frame.
pub fn decode_nip46_onboarding(bytes: &[u8]) -> Result<Nip46OnboardingModel, String> {
    if bytes.len() < 8 || !fb::nip_46_onboarding_buffer_has_identifier(bytes) {
        return Err("missing KN46 file identifier".to_string());
    }
    let root = fb::root_as_nip_46_onboarding(bytes)
        .map_err(|e| format!("not a valid Nip46Onboarding buffer: {e}"))?;

    let mut signer_apps = Vec::new();
    if let Some(apps) = root.signer_apps() {
        signer_apps.reserve(apps.len());
        for app in apps.iter() {
            signer_apps.push(SignerAppRow {
                scheme: app.scheme().unwrap_or_default().to_string(),
                display_label: app.display_label().unwrap_or_default().to_string(),
                signer_kind: app.signer_kind().unwrap_or_default().to_string(),
            });
        }
    }
    Ok(Nip46OnboardingModel {
        signer_apps,
        stage_kind: root
            .has_stage_kind()
            .then(|| root.stage_kind().unwrap_or_default().to_string()),
        progress_message: root
            .has_progress_message()
            .then(|| root.progress_message().unwrap_or_default().to_string()),
        is_in_flight: root.is_in_flight(),
        is_failed: root.is_failed(),
        is_terminal_success: root.is_terminal_success(),
        can_cancel: root.can_cancel(),
    })
}

#[cfg(test)]
#[path = "nip46_onboarding_fb_tests.rs"]
mod tests;
