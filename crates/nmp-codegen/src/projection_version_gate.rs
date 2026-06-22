//! #1723 (epic #1719) — fail-closed **producer-version drift gate** for the
//! Tier-1 NIP-crate (and other non-`nmp-core`) projection producers.
//!
//! ## Why this exists
//!
//! The [`crate::projection_contract`] manifest is the single source for each
//! projection's `version`. For the `nmp-core` kernel + actor producers, #1849's
//! [`crate::producer_consts`] generator already collapses the producer's
//! `*_SCHEMA_VERSION` const ONTO the contract (the const is generated FROM the
//! contract, so it can never disagree). But the Tier-1 NIP-crate producers
//! (`nmp-nip17` / `nmp-nip29` / `nmp-nip51` / `nmp-nip01` / `nmp-nip02` /
//! `nmp-nip47` / `nmp-nip57`, `nmp-marmot`, `nmp-content`) do NOT depend on
//! `nmp-codegen`, so they still HAND-DECLARE their own `*_SCHEMA_VERSION` const.
//! Nothing prevented the contract's `version` from drifting away from those
//! producer consts — and it had: every NIP projection carried a `version: 0`
//! placeholder while its producer stamped a real version (e.g. `dm_inbox` = 2).
//!
//! ## What this gate does
//!
//! Until the producer-const migration reaches the NIP crates (a separate slice
//! that needs a codegen/dependency path INTO those crates — see the module-level
//! STOP note below), this gate closes the drift the other direction: it READS
//! each NIP producer's `*_fb.rs`/`typed_fb.rs` source on disk and asserts the
//! `*_SCHEMA_VERSION` literal it declares EQUALS the contract's `version` for
//! that key. A future edit that bumps a producer's schema version (or the
//! contract's) without bumping the other fails this gate at commit time.
//!
//! Reading source files at test time mirrors how [`crate::producer_consts`]'s
//! `--check` diff reads its on-disk generated files: the gate runs in the
//! `nmp-codegen` test harness with the repo checked out, so the producer sources
//! are reachable via `CARGO_MANIFEST_DIR` even though the crates aren't linked.
//!
//! ## STOP-and-report — what the FULL producer migration still needs
//!
//! Making the NIP producers DERIVE their `*_SCHEMA_VERSION` from the contract
//! (the way the `nmp-core` producers now do via [`crate::producer_consts`])
//! requires a codegen/dependency path INTO the NIP crates — they do not depend
//! on `nmp-codegen`, and `nmp-codegen` deliberately does not depend on them. That
//! is a SEPARATE slice (extend `PRODUCER_CONST_TARGETS` to emit
//! `*_producer_consts.generated.rs` into each NIP crate + have each NIP `*_fb.rs`
//! `include!` it + wire those paths into `codegen-drift.yml`). This gate is the
//! interim fail-closed guard that makes the contract authoritative NOW; it does
//! not perform that migration.

use std::path::PathBuf;

use crate::projection_contract::contract_for;

/// One NIP/host producer whose hand-declared `*_SCHEMA_VERSION` is checked
/// against its contract row.
pub struct ProducerVersionSource {
    /// The `PROJECTION_CONTRACT` key whose `version` must equal the source const.
    pub key: &'static str,
    /// The producer source file (repo-root-relative) that declares the const.
    pub source_path: &'static str,
    /// The `*_SCHEMA_VERSION` const name declared in `source_path` (e.g.
    /// `DM_INBOX_SCHEMA_VERSION`, or bare `SCHEMA_VERSION` for the
    /// single-projection wire modules).
    pub const_name: &'static str,
}

/// The Tier-1 (and other non-`nmp-core`) producer sources whose hand-declared
/// schema-version const must match the contract. Every `key` here MUST have a
/// `PROJECTION_CONTRACT` entry (the gate fails closed via [`contract_for`]).
///
/// `nmp-core` producers are intentionally absent: their consts are GENERATED
/// from the contract by [`crate::producer_consts`], so they cannot drift and are
/// covered by that generator's `--check` gate instead.
pub const PRODUCER_VERSION_SOURCES: &[ProducerVersionSource] = &[
    ProducerVersionSource {
        key: "nmp.nip17.dm_inbox",
        source_path: "crates/nmp-nip17/src/wire/dm_inbox_fb.rs",
        const_name: "DM_INBOX_SCHEMA_VERSION",
    },
    ProducerVersionSource {
        key: "nmp.nip17.dm_relay_list",
        source_path: "crates/nmp-nip17/src/wire/dm_relay_list_fb.rs",
        const_name: "DM_RELAY_LIST_SCHEMA_VERSION",
    },
    ProducerVersionSource {
        key: "nmp.nip29.group_chat",
        source_path: "crates/nmp-nip29/src/wire/group_chat_fb.rs",
        const_name: "GROUP_CHAT_SCHEMA_VERSION",
    },
    ProducerVersionSource {
        key: "nmp.nip29.discovered_groups",
        source_path: "crates/nmp-nip29/src/wire/discovered_groups_fb.rs",
        const_name: "DISCOVERED_GROUPS_SCHEMA_VERSION",
    },
    ProducerVersionSource {
        key: "nmp.nip29.group_defaults",
        source_path: "crates/nmp-nip29/src/wire/group_defaults_fb.rs",
        const_name: "GROUP_DEFAULTS_SCHEMA_VERSION",
    },
    ProducerVersionSource {
        key: "nmp.nip29.joined_groups",
        source_path: "crates/nmp-nip29/src/wire/joined_groups_fb.rs",
        const_name: "JOINED_GROUPS_SCHEMA_VERSION",
    },
    ProducerVersionSource {
        key: "nmp.nip29.group_events",
        source_path: "crates/nmp-nip29/src/wire/group_events_fb.rs",
        const_name: "GROUP_EVENTS_SCHEMA_VERSION",
    },
    ProducerVersionSource {
        key: "nmp.nip51.mute_list",
        source_path: "crates/nmp-nip51/src/wire/mute_list_fb.rs",
        const_name: "MUTE_LIST_SCHEMA_VERSION",
    },
    ProducerVersionSource {
        key: "nmp.feed.home",
        source_path: "crates/nmp-nip01/src/op_feed/typed_wire.rs",
        const_name: "OP_FEED_SCHEMA_VERSION",
    },
    ProducerVersionSource {
        key: "nmp.follow_list",
        source_path: "crates/nmp-nip02/src/wire/typed_fb.rs",
        const_name: "SCHEMA_VERSION",
    },
    ProducerVersionSource {
        key: "wallet",
        source_path: "crates/nmp-nip47/src/wire/typed_fb.rs",
        const_name: "SCHEMA_VERSION",
    },
    ProducerVersionSource {
        key: "nmp.nip57.zaps",
        source_path: "crates/nmp-nip57/src/wire/typed_fb.rs",
        const_name: "SCHEMA_VERSION",
    },
    ProducerVersionSource {
        key: "claimed_event_embeds",
        source_path: "crates/nmp-content/src/wire/embed_sidecar_fb/mod.rs",
        const_name: "SCHEMA_VERSION",
    },
    ProducerVersionSource {
        key: "nmp.marmot.snapshot",
        source_path: "crates/nmp-marmot/src/wire/snapshot_fb.rs",
        const_name: "SCHEMA_VERSION",
    },
    ProducerVersionSource {
        key: "nmp.marmot.messages",
        source_path: "crates/nmp-marmot/src/wire/messages_fb.rs",
        const_name: "SCHEMA_VERSION",
    },
];

/// Resolve the repo root from `CARGO_MANIFEST_DIR` (= `<repo>/crates/nmp-codegen`).
///
/// # Panics
/// When `CARGO_MANIFEST_DIR` does not have the expected `<repo>/crates/nmp-codegen`
/// shape — a harness misconfiguration the gate must not silently pass through.
#[must_use]
pub fn repo_root() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent() // crates/
        .and_then(|p| p.parent()) // repo root
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            panic!(
                "CARGO_MANIFEST_DIR {manifest:?} is not <repo>/crates/nmp-codegen; \
                 cannot resolve repo root for the producer-version gate"
            )
        })
}

/// Parse the `u32` value of a `<vis> const <const_name>: u32 = <N>;`
/// declaration in `source`. Returns `None` when no matching declaration is
/// found.
///
/// Deliberately a line scan, not a full Rust parse — the producer consts are
/// flat top-level declarations — but FAIL-CLOSED: the line must be a real
/// declaration (not a `//` comment), the visibility must be exactly `pub` /
/// `pub(crate)` / nothing, the identifier must match wholly, the type annotation
/// must be `: u32`, and the value must be a pure decimal literal terminated by
/// `;` (a `1 + 1` expression or a trailing `_u32` suffix yields `None`, not a
/// truncated parse). Anything that does not match this exact shape returns
/// `None`, which the gate treats as a non-match (a failure), so a malformed or
/// commented-out producer const can never silently satisfy the gate.
#[must_use]
pub fn parse_const_u32(source: &str, const_name: &str) -> Option<u32> {
    for line in source.lines() {
        let trimmed = line.trim_start();
        // Fail-closed: ignore comment lines outright (a commented-out stale
        // declaration must never be parsed as the live value).
        if trimmed.starts_with("//") {
            continue;
        }
        // Strip an exact leading visibility token, if any. Only the two forms the
        // producers use are accepted; anything else is not a recognised decl.
        let after_vis = if let Some(rest) = trimmed.strip_prefix("pub(crate) ") {
            rest
        } else if let Some(rest) = trimmed.strip_prefix("pub ") {
            rest
        } else {
            trimmed
        };
        let after_const = match after_vis.strip_prefix("const ") {
            Some(rest) => rest.trim_start(),
            None => continue,
        };
        let rest = match after_const.strip_prefix(const_name) {
            Some(rest) => rest,
            None => continue,
        };
        // Whole-identifier boundary: the char after the name must begin the type
        // annotation (`:` directly or after whitespace), never a name char.
        let after_name = rest.trim_start();
        let after_colon = match after_name.strip_prefix(':') {
            Some(rest) => rest.trim_start(),
            None => continue,
        };
        // Require the exact `u32` type so we never read a differently-typed const.
        let after_ty = match after_colon.strip_prefix("u32") {
            Some(rest) => rest.trim_start(),
            None => continue,
        };
        let after_eq = match after_ty.strip_prefix('=') {
            Some(rest) => rest.trim(),
            None => continue,
        };
        // The value must be a pure decimal literal terminated by `;` — fail
        // closed on expressions, suffixes, or anything non-literal.
        let value = match after_eq.strip_suffix(';') {
            Some(v) => v.trim(),
            None => continue,
        };
        if !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit()) {
            if let Ok(v) = value.parse::<u32>() {
                return Some(v);
            }
        }
    }
    None
}

/// Outcome of checking one producer source against its contract row.
#[derive(Debug)]
pub struct ProducerVersionCheckOutcome {
    /// The contract key checked.
    pub key: &'static str,
    /// The producer source path read.
    pub source_path: &'static str,
    /// The contract's `version` for `key`.
    pub contract_version: u32,
    /// The `*_SCHEMA_VERSION` parsed from the producer source, or `None` when the
    /// source file or const could not be read/parsed.
    pub producer_version: Option<u32>,
}

impl ProducerVersionCheckOutcome {
    /// `true` iff the producer const was found AND equals the contract version.
    #[must_use]
    pub fn matches(&self) -> bool {
        self.producer_version == Some(self.contract_version)
    }
}

/// Check every [`PRODUCER_VERSION_SOURCES`] entry against the contract under
/// `repo_root`. Returns one outcome per entry, in registry order. A missing file
/// or unparseable const yields `producer_version: None` (a non-match).
#[must_use]
pub fn check_all_producer_versions(repo_root: &std::path::Path) -> Vec<ProducerVersionCheckOutcome> {
    PRODUCER_VERSION_SOURCES
        .iter()
        .map(|src| {
            let contract_version = contract_for(src.key).version;
            let producer_version = std::fs::read_to_string(repo_root.join(src.source_path))
                .ok()
                .and_then(|s| parse_const_u32(&s, src.const_name));
            ProducerVersionCheckOutcome {
                key: src.key,
                source_path: src.source_path,
                contract_version,
                producer_version,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_source_key_has_a_contract_entry() {
        for src in PRODUCER_VERSION_SOURCES {
            // Panics if absent.
            let _ = contract_for(src.key);
        }
    }

    #[test]
    fn parse_const_u32_handles_visibilities_and_boundaries() {
        assert_eq!(parse_const_u32("pub const SCHEMA_VERSION: u32 = 2;", "SCHEMA_VERSION"), Some(2));
        assert_eq!(
            parse_const_u32("pub(crate) const X_SCHEMA_VERSION : u32 = 5 ;", "X_SCHEMA_VERSION"),
            Some(5)
        );
        assert_eq!(parse_const_u32("const SCHEMA_VERSION: u32 = 7;", "SCHEMA_VERSION"), Some(7));
        // Prefix collision must not match.
        assert_eq!(
            parse_const_u32("pub const SCHEMA_VERSION_FOO: u32 = 9;", "SCHEMA_VERSION"),
            None
        );
        assert_eq!(parse_const_u32("// nothing here", "SCHEMA_VERSION"), None);
    }

    /// FAIL-CLOSED parser: a commented-out stale declaration, a non-`u32` type, a
    /// non-literal expression, or a literal suffix all yield `None` (a gate
    /// failure) rather than a truncated/wrong parse — the weakness codex flagged.
    #[test]
    fn parse_const_u32_is_fail_closed() {
        // Commented-out stale decl must NOT be parsed.
        assert_eq!(
            parse_const_u32("// pub const SCHEMA_VERSION: u32 = 1;", "SCHEMA_VERSION"),
            None
        );
        // First (live) decl wins over a later commented one; the comment is skipped.
        let src = "// pub const SCHEMA_VERSION: u32 = 1;\npub const SCHEMA_VERSION: u32 = 2;";
        assert_eq!(parse_const_u32(src, "SCHEMA_VERSION"), Some(2));
        // Non-literal expression fails closed (no `1` truncation).
        assert_eq!(parse_const_u32("pub const SCHEMA_VERSION: u32 = 1 + 1;", "SCHEMA_VERSION"), None);
        // Suffixed literal fails closed.
        assert_eq!(parse_const_u32("pub const SCHEMA_VERSION: u32 = 1_u32;", "SCHEMA_VERSION"), None);
        // Wrong type fails closed.
        assert_eq!(parse_const_u32("pub const SCHEMA_VERSION: u8 = 1;", "SCHEMA_VERSION"), None);
        // Missing terminator fails closed.
        assert_eq!(parse_const_u32("pub const SCHEMA_VERSION: u32 = 1", "SCHEMA_VERSION"), None);
        // Unrecognised visibility fails closed.
        assert_eq!(
            parse_const_u32("pub(super) const SCHEMA_VERSION: u32 = 1;", "SCHEMA_VERSION"),
            None
        );
    }

    /// FAIL-CLOSED: every NIP/host producer's hand-declared `*_SCHEMA_VERSION`
    /// equals the contract's `version` for that projection. This is the
    /// load-bearing drift gate — bumping a producer schema version (or the
    /// contract) without the other fails here at commit time.
    #[test]
    fn producer_versions_match_contract() {
        let root = repo_root();
        let outcomes = check_all_producer_versions(&root);
        let mut failures = Vec::new();
        for o in &outcomes {
            if !o.matches() {
                let const_name = PRODUCER_VERSION_SOURCES
                    .iter()
                    .find(|s| s.key == o.key)
                    .map_or("?", |s| s.const_name);
                let producer = o
                    .producer_version
                    .map_or_else(|| "<not found>".to_string(), |v| v.to_string());
                failures.push(format!(
                    "{key:?}: contract version {cv} != producer {producer} \
                     ({path}::{const_name}). Update the PROJECTION_CONTRACT entry or the \
                     producer const so they agree.",
                    key = o.key,
                    cv = o.contract_version,
                    path = o.source_path,
                ));
            }
        }
        assert!(
            failures.is_empty(),
            "PROJECTION_CONTRACT version drift from NIP producers:\n{}",
            failures.join("\n")
        );
    }
}
