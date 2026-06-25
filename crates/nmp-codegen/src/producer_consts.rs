//! #1723 (epic #1719) — generator for the per-projection **producer constants**
//! (`*_SCHEMA_ID` / `*_FILE_IDENTIFIER` / `*_SCHEMA_VERSION`) the `nmp-core`
//! typed-projection codecs (`kernel/typed_projections/*_fb.rs`,
//! `actor/typed_projections/*_fb.rs`) previously hand-declared.
//!
//! ## Why this exists
//!
//! Each `*_fb.rs` used to spell its own `schema_id` / `file_identifier` /
//! `schema_version` triple by hand. Those are the SAME neutral wire-identity
//! facts the [`crate::projection_contract`] manifest already owns
//! (`ProjectionContract::schema_id` / `file_identifier` / `version`). #1831
//! collapsed the kernel built-in key set, the revision dependency table, and the
//! presence-policy sets onto the contract; this module is the #1723 follow-up
//! that collapses the producer constants too — the producer crate no longer
//! re-states facts the contract holds.
//!
//! ## Mechanism (same shape as [`crate::rust_builtin_keys`])
//!
//! Each producer module gets a tiny `<name>_producer_consts.generated.rs`
//! rendered from its `PROJECTION_CONTRACT` entry and `include!`d in place of the
//! deleted hand-declared block. The const NAMES and per-const VISIBILITY are
//! `nmp-core`-local presentation facts (not neutral), so they live in the
//! [`PRODUCER_CONST_TARGETS`] registry here rather than in the contract. A drift
//! gate (`nmp gen producer-consts --check`, run in
//! `.github/workflows/codegen-drift.yml`) fails any PR whose checked-in files
//! differ from a fresh render, so the producer constants can never silently
//! diverge from the contract.
//!
//! ## Scope (#1723 — completes the producer migration)
//!
//! The `nmp-core` kernel + actor `*_fb.rs` producers (the #1849 first slice) AND
//! the Tier-1 NIP-crate producers (`nmp-nip17` / `nmp-nip29` / `nmp-nip51`). Every
//! projection in this set has a matching `PROJECTION_CONTRACT` entry. The generator
//! writes by repo-root-relative path, so it can emit into the NIP crates' source
//! trees even though those crates do NOT depend on `nmp-codegen` — each NIP
//! `*_fb.rs` simply `include!`s the committed generated file, exactly as the
//! `nmp-core` codecs do (no build-dep cycle). This retires the interim
//! [`crate::projection_version_gate`] for the migrated keys: their
//! `*_SCHEMA_VERSION` is now GENERATED from the contract (covered by this
//! generator's `--check` gate) instead of hand-declared.

use std::path::Path;

use crate::projection_contract::contract_for;

/// Rust visibility for an emitted const. The producer modules mix `pub`
/// (re-exported through `typed_projections/mod.rs`) and `pub(crate)`
/// (crate-internal use only); the generator preserves whatever the
/// hand-declared block used so this is a pure no-op refactor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Vis {
    /// `pub const …`.
    Public,
    /// `pub(crate) const …`.
    Crate,
}

impl Vis {
    fn keyword(self) -> &'static str {
        match self {
            Vis::Public => "pub",
            Vis::Crate => "pub(crate)",
        }
    }
}

/// One producer module's const-generation target: which contract key sources the
/// values, the `<PREFIX>_*` const-name prefix, the per-const visibility, and the
/// generated-file path (relative to the repo root) the module `include!`s.
pub struct ProducerConstTarget {
    /// The `PROJECTION_CONTRACT` key whose `schema_id` / `file_identifier` /
    /// `version` columns supply the values.
    pub key: &'static str,
    /// The `SCREAMING_SNAKE` const-name prefix (e.g. `PROFILE` →
    /// `PROFILE_SCHEMA_ID`).
    pub const_prefix: &'static str,
    /// Visibility of the `*_SCHEMA_ID` const.
    pub schema_id_vis: Vis,
    /// Visibility of the `*_FILE_IDENTIFIER` const.
    pub file_identifier_vis: Vis,
    /// Visibility of the `*_SCHEMA_VERSION` const.
    pub schema_version_vis: Vis,
    /// Generated-file path the producer module `include!`s, relative to the repo
    /// root.
    pub out_path: &'static str,
}

/// The producer-const generation targets — the `nmp-core` kernel + actor
/// `*_fb.rs` modules whose `schema_id` / `file_identifier` / `schema_version`
/// constants are derived from the projection contract.
///
/// Every `key` here MUST have a `PROJECTION_CONTRACT` entry (the render path
/// fails closed via [`contract_for`] otherwise).
pub const PRODUCER_CONST_TARGETS: &[ProducerConstTarget] = &[
    // ── kernel built-ins (all consts `pub`, re-exported through mod.rs) ──────────
    pub_target(
        "profile",
        "PROFILE",
        "crates/nmp-core/src/kernel/typed_projections/profile_producer_consts.generated.rs",
    ),
    pub_target(
        "accounts",
        "ACCOUNTS",
        "crates/nmp-core/src/kernel/typed_projections/accounts_producer_consts.generated.rs",
    ),
    pub_target(
        "active_account",
        "ACTIVE_ACCOUNT",
        "crates/nmp-core/src/kernel/typed_projections/active_account_producer_consts.generated.rs",
    ),
    pub_target(
        "claimed_events",
        "CLAIMED_EVENTS",
        "crates/nmp-core/src/kernel/typed_projections/claimed_events_producer_consts.generated.rs",
    ),
    pub_target(
        "configured_relays",
        "CONFIGURED_RELAYS",
        "crates/nmp-core/src/kernel/typed_projections/configured_relays_producer_consts.generated.rs",
    ),
    pub_target(
        "relay_role_options",
        "RELAY_ROLE_OPTIONS",
        "crates/nmp-core/src/kernel/typed_projections/relay_role_options_producer_consts.generated.rs",
    ),
    pub_target(
        "settings_hub",
        "SETTINGS_HUB",
        "crates/nmp-core/src/kernel/typed_projections/settings_hub_producer_consts.generated.rs",
    ),
    pub_target(
        "publish_queue",
        "PUBLISH_QUEUE",
        "crates/nmp-core/src/kernel/typed_projections/publish_queue_producer_consts.generated.rs",
    ),
    pub_target(
        "publish_outbox",
        "PUBLISH_OUTBOX",
        "crates/nmp-core/src/kernel/typed_projections/publish_outbox_producer_consts.generated.rs",
    ),
    pub_target(
        "outbox_summary",
        "OUTBOX_SUMMARY",
        "crates/nmp-core/src/kernel/typed_projections/outbox_summary_producer_consts.generated.rs",
    ),
    pub_target(
        "action_results",
        "ACTION_RESULTS",
        "crates/nmp-core/src/kernel/typed_projections/action_results_producer_consts.generated.rs",
    ),
    pub_target(
        "signed_events",
        "SIGNED_EVENTS",
        "crates/nmp-core/src/kernel/typed_projections/signed_events_producer_consts.generated.rs",
    ),
    pub_target(
        "action_stages",
        "ACTION_STAGES",
        "crates/nmp-core/src/kernel/typed_projections/action_stages_producer_consts.generated.rs",
    ),
    pub_target(
        "action_lifecycle",
        "ACTION_LIFECYCLE",
        "crates/nmp-core/src/kernel/typed_projections/action_lifecycle_producer_consts.generated.rs",
    ),
    pub_target(
        "relay_diagnostics",
        "RELAY_DIAGNOSTICS",
        "crates/nmp-core/src/kernel/typed_projections/relay_diagnostics_producer_consts.generated.rs",
    ),
    // ── actor Tier-1 registrations (SCHEMA_ID `pub`, others `pub(crate)`) ────────
    ProducerConstTarget {
        key: "bunker_handshake",
        const_prefix: "BUNKER_HANDSHAKE",
        schema_id_vis: Vis::Public,
        file_identifier_vis: Vis::Crate,
        schema_version_vis: Vis::Crate,
        out_path: "crates/nmp-core/src/actor/typed_projections/bunker_handshake_producer_consts.generated.rs",
    },
    ProducerConstTarget {
        key: "nip46_onboarding",
        const_prefix: "NIP46_ONBOARDING",
        schema_id_vis: Vis::Public,
        file_identifier_vis: Vis::Crate,
        schema_version_vis: Vis::Crate,
        out_path: "crates/nmp-core/src/actor/typed_projections/nip46_onboarding_producer_consts.generated.rs",
    },
    ProducerConstTarget {
        key: "signer_state",
        const_prefix: "SIGNER_STATE",
        schema_id_vis: Vis::Public,
        file_identifier_vis: Vis::Crate,
        schema_version_vis: Vis::Crate,
        out_path: "crates/nmp-core/src/actor/typed_projections/signer_state_producer_consts.generated.rs",
    },
    // ── Tier-1 NIP-crate producers (all consts `pub`) ────────────────────────────
    // These crates do NOT depend on `nmp-codegen`; the generator writes by
    // repo-relative path and each `*_fb.rs` `include!`s the committed file (no
    // build-dep cycle). The contract `key` is the dotted projection key; the
    // const PREFIX is the producer's `SCREAMING_SNAKE` name.
    pub_target(
        "nmp.nip17.dm_inbox",
        "DM_INBOX",
        "crates/nmp-nip17/src/wire/dm_inbox_producer_consts.generated.rs",
    ),
    pub_target(
        "nmp.nip17.dm_relay_list",
        "DM_RELAY_LIST",
        "crates/nmp-nip17/src/wire/dm_relay_list_producer_consts.generated.rs",
    ),
    pub_target(
        "nmp.nip29.group_chat",
        "GROUP_CHAT",
        "crates/nmp-nip29/src/wire/group_chat_producer_consts.generated.rs",
    ),
    pub_target(
        "nmp.nip29.discovered_groups",
        "DISCOVERED_GROUPS",
        "crates/nmp-nip29/src/wire/discovered_groups_producer_consts.generated.rs",
    ),
    pub_target(
        "nmp.nip29.group_defaults",
        "GROUP_DEFAULTS",
        "crates/nmp-nip29/src/wire/group_defaults_producer_consts.generated.rs",
    ),
    pub_target(
        "nmp.nip29.joined_groups",
        "JOINED_GROUPS",
        "crates/nmp-nip29/src/wire/joined_groups_producer_consts.generated.rs",
    ),
    pub_target(
        "nmp.nip51.mute_list",
        "MUTE_LIST",
        "crates/nmp-nip51/src/wire/mute_list_producer_consts.generated.rs",
    ),
];

/// Build an all-`pub` kernel target (the common case).
const fn pub_target(
    key: &'static str,
    const_prefix: &'static str,
    out_path: &'static str,
) -> ProducerConstTarget {
    ProducerConstTarget {
        key,
        const_prefix,
        schema_id_vis: Vis::Public,
        file_identifier_vis: Vis::Public,
        schema_version_vis: Vis::Public,
        out_path,
    }
}

/// Render one target's `*_SCHEMA_ID` / `*_FILE_IDENTIFIER` / `*_SCHEMA_VERSION`
/// const trio from its `PROJECTION_CONTRACT` entry.
///
/// `file_identifier` is the contract's 4-char ASCII `&str`; it is emitted as the
/// `&[u8; 4]` byte-string literal the producer modules use. The byte literal is
/// derived purely by quoting the contract string (`"KPRF"` → `b"KPRF"`), so the
/// generated value can never disagree with the contract's `file_identifier`.
///
/// # Panics
/// When `target.key` has no `PROJECTION_CONTRACT` entry, or its
/// `file_identifier` is not exactly 4 ASCII bytes (an invalid FlatBuffers file
/// identifier — a manifest authoring error caught at render time).
#[must_use]
pub fn render_producer_consts(target: &ProducerConstTarget) -> String {
    let c = contract_for(target.key);
    let fid = c.file_identifier;
    assert!(
        fid.len() == 4 && fid.is_ascii(),
        "PROJECTION_CONTRACT entry for {:?} has file_identifier {fid:?}, which is \
         not exactly 4 ASCII bytes — FlatBuffers file identifiers are 4 bytes",
        target.key
    );
    let prefix = target.const_prefix;
    let mut out = String::new();
    out.push_str(&format!(
        "// @generated by `cargo run -p nmp-codegen -- gen producer-consts`. DO NOT EDIT.\n\
         //\n\
         // Source of truth: crates/nmp-codegen/src/projection_contract.rs\n\
         //   (PROJECTION_CONTRACT entry for key {key:?}: schema_id / file_identifier /\n\
         //   version). Regenerate + verify drift via\n\
         //   `.github/workflows/codegen-drift.yml` (gen producer-consts --check).\n",
        key = target.key,
    ));
    out.push_str(&format!(
        "/// Stable schema identifier carried in the typed-projection envelope. Equals the\n\
         /// snapshot key (ADR-0037 shared-keyspace contract).\n\
         {schema_id_vis} const {prefix}_SCHEMA_ID: &str = {schema_id:?};\n",
        schema_id_vis = target.schema_id_vis.keyword(),
        schema_id = c.schema_id,
    ));
    out.push_str(&format!(
        "/// FlatBuffers file identifier embedded in every buffer this module emits.\n\
         {file_identifier_vis} const {prefix}_FILE_IDENTIFIER: &[u8; 4] = b{fid:?};\n",
        file_identifier_vis = target.file_identifier_vis.keyword(),
    ));
    out.push_str(&format!(
        "/// Wire schema version. Bump on any breaking change to this projection's `.fbs`.\n\
         {schema_version_vis} const {prefix}_SCHEMA_VERSION: u32 = {version};\n",
        schema_version_vis = target.schema_version_vis.keyword(),
        version = c.version,
    ));
    out
}

/// Outcome of a `--check` run. Mirrors [`crate::rust_builtin_keys::BuiltinKeysCheckOutcome`].
#[derive(Debug)]
pub struct ProducerConstsCheckOutcome {
    /// The repo-relative path checked.
    pub out_path: &'static str,
    /// `true` when the on-disk file matches the freshly-rendered output.
    pub up_to_date: bool,
    /// First differing line (1-based) when stale; `None` when up-to-date OR when
    /// the file doesn't exist.
    pub first_diff_line: Option<usize>,
}

/// Write every target's generated const file under `repo_root`.
///
/// # Errors
/// Filesystem I/O failures.
pub fn generate_all_producer_consts(repo_root: &Path) -> std::io::Result<()> {
    for target in PRODUCER_CONST_TARGETS {
        let rendered = render_producer_consts(target);
        let out = repo_root.join(target.out_path);
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&out, rendered)?;
    }
    Ok(())
}

/// Diff every target's freshly-rendered file against its on-disk file under
/// `repo_root`. A missing file is reported as stale. Returns one outcome per
/// target, in `PRODUCER_CONST_TARGETS` order.
///
/// # Errors
/// Filesystem I/O failures other than NotFound.
pub fn check_all_producer_consts(
    repo_root: &Path,
) -> std::io::Result<Vec<ProducerConstsCheckOutcome>> {
    let mut outcomes = Vec::with_capacity(PRODUCER_CONST_TARGETS.len());
    for target in PRODUCER_CONST_TARGETS {
        let rendered = render_producer_consts(target);
        let out = repo_root.join(target.out_path);
        let actual = match std::fs::read_to_string(&out) {
            Ok(s) => s,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                outcomes.push(ProducerConstsCheckOutcome {
                    out_path: target.out_path,
                    up_to_date: false,
                    first_diff_line: None,
                });
                continue;
            }
            Err(err) => return Err(err),
        };
        if actual == rendered {
            outcomes.push(ProducerConstsCheckOutcome {
                out_path: target.out_path,
                up_to_date: true,
                first_diff_line: None,
            });
        } else {
            outcomes.push(ProducerConstsCheckOutcome {
                out_path: target.out_path,
                up_to_date: false,
                first_diff_line: crate::diff_report::first_diff_or_length(&actual, &rendered),
            });
        }
    }
    Ok(outcomes)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every target key resolves to a contract entry (fail-closed render).
    #[test]
    fn every_target_key_has_a_contract_entry() {
        for target in PRODUCER_CONST_TARGETS {
            // Panics if absent.
            let _ = render_producer_consts(target);
        }
    }

    /// The render reproduces the historic hand-declared profile block exactly.
    #[test]
    fn profile_render_matches_known_values() {
        let target = PRODUCER_CONST_TARGETS
            .iter()
            .find(|t| t.key == "profile")
            .expect("profile target present");
        let r = render_producer_consts(target);
        assert!(r.contains("pub const PROFILE_SCHEMA_ID: &str = \"profile\";"));
        assert!(r.contains("pub const PROFILE_FILE_IDENTIFIER: &[u8; 4] = b\"KPRF\";"));
        assert!(r.contains("pub const PROFILE_SCHEMA_VERSION: u32 = 2;"));
    }

    /// The actor targets keep the `pub` SCHEMA_ID / `pub(crate)` other-two split.
    #[test]
    fn actor_targets_preserve_pub_crate_visibility() {
        let target = PRODUCER_CONST_TARGETS
            .iter()
            .find(|t| t.key == "signer_state")
            .expect("signer_state target present");
        let r = render_producer_consts(target);
        assert!(r.contains("pub const SIGNER_STATE_SCHEMA_ID: &str = \"signer_state\";"));
        assert!(r.contains("pub(crate) const SIGNER_STATE_FILE_IDENTIFIER: &[u8; 4] = b\"KSST\";"));
        assert!(r.contains("pub(crate) const SIGNER_STATE_SCHEMA_VERSION: u32 = 1;"));
    }

    /// The migrated NIP-crate targets render their dotted-key SCHEMA_ID, NIP
    /// file identifier, and the contract version (all `pub`).
    #[test]
    fn nip_targets_render_dotted_schema_id_and_contract_version() {
        let target = PRODUCER_CONST_TARGETS
            .iter()
            .find(|t| t.key == "nmp.nip17.dm_inbox")
            .expect("dm_inbox target present");
        let r = render_producer_consts(target);
        assert!(r.contains("pub const DM_INBOX_SCHEMA_ID: &str = \"nmp.nip17.dm_inbox\";"));
        assert!(r.contains("pub const DM_INBOX_FILE_IDENTIFIER: &[u8; 4] = b\"NDMI\";"));
        assert!(r.contains("pub const DM_INBOX_SCHEMA_VERSION: u32 = 2;"));
    }

    /// Render is deterministic.
    #[test]
    fn render_is_stable() {
        for target in PRODUCER_CONST_TARGETS {
            assert_eq!(render_producer_consts(target), render_producer_consts(target));
        }
    }
}
