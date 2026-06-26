//! D26 — no ambient authority in protocol/command code (Workstream D item 7;
//! the K2 + D6 capability-honesty lock-in).
//!
//! D6 split the wide `AppHost` god-trait into narrow single-concern
//! registration/capability traits (`IngestParserRegistrar`,
//! `RelayConnectedHookRegistrar`, `HostCapabilities`, …) so a reusable
//! protocol/command module receives ONLY the surface it actually uses (D0
//! capability honesty). `AppHost` survives only as a **composition super-trait**
//! that the composition root (`nmp-defaults` / `nmp-ffi` wiring) may name — never
//! a narrow protocol consumer. In parallel, secret signing material is reached
//! through the signer-session port / a narrow capability (`ctx.sign_event_for_account`,
//! the `LocalSignerAccess` port), never by a protocol command pulling the raw
//! `nostr::Keys` out of an ambient accessor.
//!
//! D26 makes both permanent: protocol/command code cannot reference
//! `active_local_keys` or the broad `AppHost`; signing goes through the
//! signer-session port only. It is **D21-adjacent**: D21 bans ambient-authority
//! *statics*; D26 bans ambient-authority *type/accessor* references in
//! protocol-command code.
//!
//! ## What this bans (two tokens, in protocol/command code)
//!
//! 1. **`AppHost`** as a type / trait-bound. A reusable protocol module that
//!    names `AppHost` (e.g. `fn register(host: &impl AppHost)` or
//!    `use crate::substrate::AppHost`) is taking the whole ~13-trait god-surface
//!    instead of the narrow registrar(s) it uses — the exact D6 regression.
//!    Boundary-anchored on both sides, so `AppHostImpl`, `MyAppHost`, and
//!    `HostCapabilities` never false-positive — only the bare `AppHost` token.
//!
//! 2. **`active_local_keys`** — the raw signing-key accessor. A protocol command
//!    reaching `ctx.active_local_keys()` pulls the active account's raw
//!    `nostr::Keys` instead of signing through the signer-session port
//!    (`sign_event_for_account`), which is also `None` for NIP-46 bunker accounts
//!    — exactly the V-78 signer-transparency defect. Boundary-anchored so a
//!    longer identifier ending in the token does not false-positive.
//!
//! Both scans run only on the **code portion** of a line (everything before a
//! trailing `//` comment), so a doc/inline comment naming `AppHost` — the common
//! "this crate does NOT take `AppHost`" annotation in the NIP crates — never
//! fires.
//!
//! ## Scope (`app_host_in_scope` / `active_local_keys_in_scope`)
//!
//! The two tokens have different legitimate-definition footprints, so each has
//! its own scope predicate:
//!
//! - **`AppHost`** — the protocol/command surface: the reusable protocol &
//!   routing crates ([`PROTOCOL_CRATES`] + every `nmp-nipNN` crate) PLUS the
//!   `nmp-core` protocol-command modules (`substrate/protocol*`,
//!   `actor/commands/`). The `AppHost` DEFINITION (`substrate/app_host/`) and the
//!   composition root (`nmp-defaults`, `nmp-ffi`) are NOT in scope — they
//!   legitimately name `AppHost`. Master is green: every in-scope `AppHost`
//!   reference today lives in a doc comment.
//!
//! - **`active_local_keys`** — the protocol-command IMPLEMENTATION crates only
//!   ([`PROTOCOL_CRATES`] + `nmp-nipNN`), i.e. where `ProtocolCommand::run`
//!   bodies live. `nmp-core` is deliberately NOT in scope: it HOSTS the
//!   legitimate capability-port DEFINITION (the `LocalSignerAccess` trait and the
//!   `ProtocolCommandContext::active_local_keys` accessor in `substrate/protocol*`),
//!   the `IdentityState` accessor, and the kernel dispatch plumbing that
//!   populates the active-keys slot. Removing the accessor FROM the port itself
//!   is the SEPARATE, not-yet-landed plan Workstream-D item 5; D26 locks the
//!   implementation surface so that once item 5 lands, no command can have
//!   re-grown a reach for it. Master is green: no production protocol command
//!   calls `active_local_keys` (only `#[cfg(test)]` test doubles, which are
//!   exempt).
//!
//! ## Exemptions
//!
//! - Doc/line comments (`is_comment`) and trailing `//` comments (stripped from
//!   the code portion) — never fire.
//! - `#[cfg(test)]` bodies (`in_test_cfg`) and test-only files
//!   (`d6::file_is_test_only`, handled in the driver) — test doubles freely
//!   implement `LocalSignerAccess`.
//! - Per-line `// doctrine-allow: D26 — reason` opt-out, REASON-REQUIRED (the
//!   shared [`crate::allow::line_allows_with_reason`] parser, the D10/D21/F idiom;
//!   a bare `// doctrine-allow: D26` does NOT silence).
//! - The doctrine-lint binary's own source tree (its string constants contain the
//!   banned tokens — meta-false-positives on broad sweeps).
//!
//! ## Heuristic scope (regression backstop, NOT a formal proof)
//!
//! D26 is a token-grep regression BACKSTOP. It catches the normal source forms of
//! the two bans. A deliberately obfuscated reach — `AppHost` aliased through a
//! re-export, or raw keys plumbed through an intermediate binding — is OUT OF
//! SCOPE and a code-review concern, not something a line-based lint chases.

use std::path::Path;

pub const ID: &str = "D26";

/// Reusable protocol / routing crates whose `src/` trees are the protocol-command
/// surface D26 guards. Every `nmp-nipNN` crate is also in scope (matched by the
/// `nmp-nip` prefix); these are the non-`nip` protocol/command crates.
const PROTOCOL_CRATES: &[&str] = &[
    "nmp-marmot",
    "nmp-blossom",
    "nmp-nwc",
    "nmp-router",
    "nmp-wot",
    "nmp-content",
    "nmp-feed",
];

/// True iff `path` is the doctrine-lint binary's own source tree (its string
/// constants contain the banned tokens).
fn is_lint_source(s: &str) -> bool {
    s.contains("/bin/doctrine-lint/")
}

/// True iff `path` lives inside a reusable protocol / routing crate's `src/`
/// tree — a `nmp-nipNN` crate, or one of [`PROTOCOL_CRATES`]. This is the
/// protocol-command IMPLEMENTATION surface (where `ProtocolCommand::run` bodies
/// live). `nmp-core` is intentionally excluded (it hosts the framework + ports).
fn is_protocol_crate(s: &str) -> bool {
    if s.contains("crates/nmp-nip") && s.contains("/src/") {
        return true;
    }
    PROTOCOL_CRATES
        .iter()
        .any(|c| s.contains(&format!("crates/{}/src/", c)))
}

/// True iff `path` is an `nmp-core` protocol-command module — the protocol
/// command framework (`substrate/protocol*`) or the actor command handlers
/// (`actor/commands/`). These take narrow traits, never `AppHost`.
fn is_core_command_module(s: &str) -> bool {
    s.contains("crates/nmp-core/src/substrate/protocol")
        || s.contains("crates/nmp-core/src/actor/commands/")
}

/// True iff the `AppHost` ban should scan `path`: the protocol-command surface
/// (reusable protocol crates + `nmp-core` protocol-command modules). Never the
/// `AppHost` DEFINITION (`substrate/app_host/`), the composition root, or the
/// lint binary itself.
pub fn app_host_in_scope(path: &Path) -> bool {
    let s = path.to_string_lossy().replace('\\', "/");
    if is_lint_source(&s) {
        return false;
    }
    if s.contains("crates/nmp-core/src/substrate/app_host") {
        return false;
    }
    is_protocol_crate(&s) || is_core_command_module(&s)
}

/// True iff the `active_local_keys` ban should scan `path`: the protocol-command
/// IMPLEMENTATION crates only. `nmp-core` is excluded — it hosts the legitimate
/// `LocalSignerAccess` port / `ProtocolCommandContext` accessor / `IdentityState`
/// accessor / dispatch plumbing (the surface plan Workstream-D item 5 removes).
pub fn active_local_keys_in_scope(path: &Path) -> bool {
    let s = path.to_string_lossy().replace('\\', "/");
    if is_lint_source(&s) {
        return false;
    }
    is_protocol_crate(&s)
}

/// The code portion of a line — everything before a trailing `//` comment — so a
/// banned token mentioned only in an inline comment never fires. Full-line doc
/// comments are already filtered by the walker's `is_comment`.
fn code_portion(line: &str) -> &str {
    match line.find("//") {
        Some(i) => &line[..i],
        None => line,
    }
}

/// True iff byte `idx` in `bytes` is an identifier char (`[A-Za-z0-9_]`).
fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// 1-indexed columns of every boundary-anchored occurrence of `token` in `code`:
/// the char immediately before AND after the match must not be an identifier
/// char, so `AppHostImpl` / `MyAppHost` / `event_active_local_keys` do not match
/// — only the bare token does.
fn boundary_anchored_cols(code: &str, token: &str) -> Vec<usize> {
    let bytes = code.as_bytes();
    let mut cols = Vec::new();
    let mut start = 0;
    while let Some(rel) = code[start..].find(token) {
        let abs = start + rel;
        start = abs + token.len();
        let left_ok = abs == 0 || !is_ident_byte(bytes[abs - 1]);
        let after = abs + token.len();
        let right_ok = after >= bytes.len() || !is_ident_byte(bytes[after]);
        if left_ok && right_ok {
            cols.push(abs + 1);
        }
    }
    cols
}

fn app_host_message() -> String {
    "`AppHost` named in protocol/command code violates D26 (Workstream D item 7 / \
     D6 lock-in). `AppHost` is the composition super-trait — the union of every \
     narrow registration/capability trait — and only the composition root \
     (`nmp-defaults` / `nmp-ffi` wiring) may name it. A reusable protocol module \
     taking `AppHost` receives the whole god-surface instead of the narrow \
     registrar(s) it actually uses (the D0 capability-honesty regression D6 \
     deleted)"
        .to_string()
}

fn app_host_suggested() -> String {
    "take the specific narrow trait(s) you use — `&impl IngestParserRegistrar`, \
     `&impl RelayConnectedHookRegistrar`, `&impl HostCapabilities`, … — never the \
     wide `AppHost`; the composition root is the one place that wires the whole \
     surface"
        .to_string()
}

fn active_local_keys_message() -> String {
    "`active_local_keys` reached in protocol/command code violates D26 \
     (Workstream D item 7 / signer transparency). A protocol command must NOT \
     pull the active account's raw `nostr::Keys` out of an ambient accessor — \
     that is `None` for NIP-46 bunker accounts and reintroduces the V-78 \
     signer-backend leak. Signing/encryption goes through the signer-session port \
     only"
        .to_string()
}

fn active_local_keys_suggested() -> String {
    "sign/encrypt through the signer-session port — `ctx.sign_event_for_account(..)` \
     / `ctx.nip44_encrypt_for_account(..)` — pinned to `ctx.active_account_pubkey()`, \
     so the signer backend (local nsec vs NIP-46 bunker) stays invisible to the \
     protocol worker"
        .to_string()
}

/// Returns `(col, message, suggested)` for each banned token on `line`.
/// `app_host_scope` / `alk_scope` gate the two tokens independently (they have
/// different scope footprints); `is_comment` / `in_test_cfg` suppress all.
pub fn check(
    line: &str,
    app_host_scope: bool,
    alk_scope: bool,
    is_comment: bool,
    in_test_cfg: bool,
) -> Vec<(usize, String, String)> {
    if is_comment || in_test_cfg {
        return Vec::new();
    }
    let code = code_portion(line);
    let mut hits = Vec::new();
    if app_host_scope {
        for col in boundary_anchored_cols(code, "AppHost") {
            hits.push((col, app_host_message(), app_host_suggested()));
        }
    }
    if alk_scope {
        for col in boundary_anchored_cols(code, "active_local_keys") {
            hits.push((col, active_local_keys_message(), active_local_keys_suggested()));
        }
    }
    hits.sort_by_key(|(c, _, _)| *c);
    hits
}

#[cfg(test)]
#[path = "d26/tests.rs"]
mod tests;
