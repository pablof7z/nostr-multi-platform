//! D21 — no ambient authority (K2 / ADR-0072 §D6 regression gate).
//!
//! Keystone K2 (instance-scoped registration, ADR-0072) deleted the five
//! process-global mutable singletons that the old TYPE-registered extension
//! seams forced stateful modules to reach through:
//!
//! - `ACTIVE_WALLET_RUNTIME: OnceLock<WalletRuntimeHandle>` (nmp-nip47) — rung 5.2
//! - the bunker `HOOK: OnceLock<RwLock<Option<BunkerHookFn>>>` (nmp-core) — rung 5.3
//! - the NIP-55 `HOOK: OnceLock<RwLock<Option<ExternalSignerHookFn>>>` (nmp-core) — rung 5.3
//!
//! …plus `ProtocolCommandContext::kernel_mut()` (rung 5.5). Per-app state now
//! lives as INSTANCE FIELDS threaded by `Arc`-slot from the composition root
//! (the `ActiveAccountSlot`/`EventStoreSlot`/`BunkerHookSlot`/… pattern), so two
//! `NmpApp` instances in one process never share authority. D21 locks that in:
//! it makes a future PR unable to silently reintroduce ambient authority.
//!
//! ## What this bans
//!
//! A module-level (or block-level) `static` / `OnceLock` / `Lazy` /
//! `lazy_static!` declaration that holds **non-const, mutable or
//! interior-mutable, process-wide state conferring authority** — specifically a
//! static whose type is:
//!
//! - `Mutex<…>` / `RwLock<…>` wrapping real state (a `Mutex<()>` serialization
//!   token holds no state and is allowed),
//! - `AtomicPtr<…>`,
//! - `OnceLock<…>` / `Lazy<…>` / a `lazy_static! { static ref … }` wrapping a
//!   handle / runtime / sender / hook / `Arc` / `Mutex` / `RwLock` / registry —
//!   anything that is NOT a plain read-once config value.
//!
//! The goal state is an **empty** allowlist: ZERO un-allowlisted occurrences.
//!
//! ## Type-scoping (instead of a justification allowlist)
//!
//! Rather than seed a reason-string allowlist with the two benign residuals,
//! D21 is **type-scoped**: `OnceLock<…>` / `Lazy<…>` of a *plain-config* type is
//! NOT authority and never fires. Plain-config = `bool`, integer / float / char,
//! `&str` / `String`, `PathBuf`, `Duration`, `Regex`, `()`, and `Option<…>` /
//! tuples composed only of those. Everything else an `OnceLock`/`Lazy` can wrap
//! — a handle, runtime, sender, hook, `Arc<…>`, `Mutex<…>`, `RwLock<…>`, a
//! registry `HashMap`, or any unrecognized non-config identifier — is treated as
//! authority and DOES fire (a `OnceLock<WalletRuntimeHandle>` is NOT excluded).
//!
//! This excludes exactly the two read-once-config residuals and nothing more:
//! - `crates/nmp-core/src/kernel/wire_log.rs` `static ENABLED: OnceLock<bool>`
//!   (debug wire-log toggle, read-once `Copy` config), and
//! - `crates/nmp-network/src/relay_worker/socket_io.rs`
//!   `static LOG_PATH: OnceLock<Option<PathBuf>>` (debug log path, read-once config).
//!
//! Type-scoping is simpler and harder to abuse than a reason allowlist: a
//! reviewer can verify the exclusion mechanically from the declared type, and a
//! plausible-sounding reason string can never launder a `OnceLock<SomeHandle>`
//! back in. The per-line `// doctrine-allow: D21 — reason` escape hatch (with a
//! mandatory written reason, the D10 tightened-parser idiom) remains for the
//! genuinely-justified residual that is NOT plain config.
//!
//! ## Scope (`file_in_scope`)
//!
//! Path-scoped to the K2 blast-radius crates' `src/` trees — the crates that
//! hosted the deleted globals (nmp-nip47, nmp-core) plus the two
//! residuals (nmp-core, nmp-network) and the signer crates the bunker/NIP-55
//! authority migrated to:
//! `nmp-core`, `nmp-nip47`, `nmp-network`, `nmp-nip46-runtime`,
//! `nmp-signers`, `nmp-signer-iface`. This is the regression surface that
//! matters; the rule engine is workspace-capable but the gate watches where K2
//! operated.
//!
//! ## Exemptions
//!
//! - Doc/line comments (`is_comment`) — skipped.
//! - `#[cfg(test)]` module bodies (`in_test_cfg`) and test-only files
//!   (`d6::file_is_test_only`, handled in the `main.rs` driver) — test doubles
//!   and serialization locks never ship.
//! - Per-line `// doctrine-allow: D21 — reason` opt-out. Like D10, D21 REQUIRES
//!   a written reason (a bare `// doctrine-allow: D21` does NOT silence it) so
//!   every escape carries an auditable justification.
//! - The doctrine-lint binary's own source tree (its string constants contain
//!   the banned tokens — meta-false-positives on broad sweeps).

use std::path::Path;

pub const ID: &str = "D21";

/// K2 blast-radius crates D21 guards. A file is in scope iff its path contains
/// `crates/<name>/src/` for one of these names.
const K2_CRATES: &[&str] = &[
    "nmp-core",
    "nmp-nip47",
    "nmp-network",
    "nmp-nip46-runtime",
    "nmp-signers",
    "nmp-signer-iface",
];

/// Plain-config inner types an `OnceLock<…>` / `Lazy<…>` may wrap WITHOUT being
/// ambient authority — read-once `Copy`/owned config, not a handle that confers
/// authority. The match is on the inner type's leading identifier.
const PLAIN_CONFIG_TYPES: &[&str] = &[
    "bool", "char", "str", "String", "PathBuf", "Path", "Duration", "Regex", "u8", "u16", "u32",
    "u64", "u128", "usize", "i8", "i16", "i32", "i64", "i128", "isize", "f32", "f64",
];

/// Inner-type fragments that ALWAYS denote authority even inside an
/// `OnceLock`/`Lazy` — a handle/runtime/sender/hook/shared-pointer/lock/registry.
/// Used to keep `Option<Sender<…>>` / `Arc<…>` from ever being mistaken for
/// plain config.
const AUTHORITY_FRAGMENTS: &[&str] = &[
    "Mutex",
    "RwLock",
    "Arc",
    "Rc<",
    "Cell",
    "RefCell",
    "Sender",
    "Receiver",
    "Handle",
    "Runtime",
    "Hook",
    "Driver",
    "Broker",
    "AtomicPtr",
    "HashMap",
    "BTreeMap",
    "Box<dyn",
    "dyn ",
];

/// True iff D21 should scan `path`: it lives inside a K2 blast-radius crate's
/// `src/` tree. Never fires in the doctrine-lint binary itself.
pub fn file_in_scope(path: &Path) -> bool {
    let s = path.to_string_lossy().replace('\\', "/");
    if s.contains("/bin/doctrine-lint/") {
        return false;
    }
    K2_CRATES
        .iter()
        .any(|c| s.contains(&format!("crates/{}/src/", c)))
}

/// Extract the type expression of a `static NAME: <TYPE> = …;` declaration.
/// Returns the substring between the first `:` after the static name and the
/// `=` (or end of line if the initializer is on a later line). Returns `None`
/// if the line is not a static declaration.
fn static_type<'a>(decl_tail: &'a str) -> Option<&'a str> {
    // decl_tail starts just after `static ` (or `static ref ` for lazy_static).
    // Shape: `NAME: TYPE = INIT;` or `NAME: TYPE;`.
    let colon = decl_tail.find(':')?;
    let after_colon = &decl_tail[colon + 1..];
    let end = after_colon.find('=').unwrap_or(after_colon.len());
    Some(after_colon[..end].trim())
}

/// Strip the outer `OnceLock<…>` / `Lazy<…>` wrapper, returning the inner type.
/// Returns `None` if `ty` is not a `OnceLock`/`Lazy`.
fn once_or_lazy_inner(ty: &str) -> Option<&str> {
    for wrapper in ["OnceLock<", "Lazy<"] {
        if let Some(pos) = ty.find(wrapper) {
            let inner = &ty[pos + wrapper.len()..];
            // Trim the trailing `>` (and any further `>` are part of nested
            // generics we keep for the authority-fragment scan).
            return Some(inner.trim_end_matches('>').trim());
        }
    }
    None
}

/// True iff the inner type of an `OnceLock`/`Lazy` is plain read-once config
/// (NOT authority). Strips a leading `Option<` so `Option<PathBuf>` is config.
fn inner_is_plain_config(inner: &str) -> bool {
    // Any authority fragment anywhere in the inner type immediately disqualifies
    // it — a `Sender<…>`, `Arc<…>`, `Mutex<…>`, registry map, trait object, etc.
    if AUTHORITY_FRAGMENTS.iter().any(|f| inner.contains(f)) {
        return false;
    }
    // Peel a single `Option<…>` wrapper (e.g. the `Option<PathBuf>` residual).
    let core = inner
        .strip_prefix("Option<")
        .map(|s| s.trim_end_matches('>'))
        .unwrap_or(inner)
        .trim()
        .trim_start_matches('&')
        .trim();
    // The empty-tuple unit `()` holds no state — a `Mutex<()>` is a pure
    // serialization token, and a `OnceLock<()>` is meaningless config.
    if core == "()" {
        return true;
    }
    // Leading identifier of the core type.
    let leading: String = core
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    PLAIN_CONFIG_TYPES.contains(&leading.as_str())
}

/// True iff a bare `Mutex<…>` / `RwLock<…>` type holds real state (authority),
/// as opposed to a `Mutex<()>` serialization token.
fn lock_holds_state(ty: &str) -> bool {
    for wrapper in ["Mutex<", "RwLock<"] {
        if let Some(pos) = ty.find(wrapper) {
            let inner = ty[pos + wrapper.len()..].trim_end_matches('>').trim();
            // `Mutex<()>` is a serialization token — no state, not authority.
            return inner != "()";
        }
    }
    false
}

/// D21-specific escape-hatch parser. Like D10's `line_allows_d10`, D21 REQUIRES
/// a written reason after a separator (`— ` or ` - `) — a bare
/// `// doctrine-allow: D21` does NOT silence the finding, so every escape
/// carries an auditable justification.
pub fn line_allows_d21(line: &str) -> bool {
    let Some(after) = line.split("// doctrine-allow:").nth(1) else {
        return false;
    };
    let (head, reason) = if let Some((h, r)) = after.split_once('—') {
        (h, r)
    } else if let Some((h, r)) = after.split_once(" - ") {
        (h, r)
    } else {
        return false;
    };
    if reason.trim().is_empty() {
        return false;
    }
    head.split(',').any(|r| {
        r.split_whitespace()
            .next()
            .map(|t| t == ID)
            .unwrap_or(false)
    })
}

/// Returns `(col, message, suggested)` for an ambient-authority static declared
/// on `line`. `is_comment` and `in_test_cfg` suppress the scan.
///
/// Detects:
/// - `static NAME: TYPE = …;` (with optional `pub` / `pub(…)` and leading
///   whitespace — module-level OR function-/block-local; both are process-wide),
/// - `static ref NAME: TYPE = …;` (the `lazy_static!` body form).
pub fn check(line: &str, is_comment: bool, in_test_cfg: bool) -> Vec<(usize, String, String)> {
    if is_comment || in_test_cfg {
        return Vec::new();
    }
    let trimmed = line.trim_start();
    // Strip an optional visibility modifier so `pub static` / `pub(crate) static`
    // are matched. We only need the substring starting at `static `.
    let static_pos = match line.find("static ") {
        Some(p) => p,
        None => return Vec::new(),
    };
    // Guard against `static` appearing inside a larger word or a string/path
    // (e.g. `lazy_static`): require the char before `static` to be a boundary.
    if static_pos > 0 {
        let prev = line.as_bytes()[static_pos - 1];
        if prev.is_ascii_alphanumeric() || prev == b'_' {
            // e.g. `lazy_static` — not a `static` keyword here. The
            // `lazy_static! { static ref … }` body line still has a real
            // `static ref` token, which this guard correctly admits because the
            // char before THAT `static` is whitespace.
            return Vec::new();
        }
    }
    let _ = trimmed;
    // The declaration tail begins after `static ` — peel an optional `ref `
    // (lazy_static form).
    let mut tail = &line[static_pos + "static ".len()..];
    // `lazy_static! { static ref NAME: T = … }` — the `ref` form. The declared
    // type `T` is the lazily-initialized value itself (no `OnceLock`/`Lazy`
    // wrapper), so authority is judged on `T` directly below.
    let is_lazy_ref = tail.starts_with("ref ");
    if let Some(rest) = tail.strip_prefix("ref ") {
        tail = rest;
    }
    let name: String = tail
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    if name.is_empty() {
        return Vec::new();
    }
    let Some(ty) = static_type(tail) else {
        return Vec::new();
    };

    let is_authority = if is_lazy_ref {
        // `lazy_static! { static ref NAME: T }` is a lazily-initialized global —
        // authority unless `T` is plain read-once config (same type-scoping as
        // an `OnceLock`/`Lazy` inner type).
        !inner_is_plain_config(ty)
    } else if let Some(inner) = once_or_lazy_inner(ty) {
        // `OnceLock<…>` / `Lazy<…>`: authority unless the inner type is plain
        // read-once config (the type-scoping that excludes the two residuals).
        !inner_is_plain_config(inner)
    } else if ty.contains("AtomicPtr<") {
        true
    } else if ty.starts_with("Mutex<") || ty.starts_with("RwLock<") {
        // A bare lock static is authority only when it holds real state — a
        // `Mutex<()>` serialization token is not.
        lock_holds_state(ty)
    } else {
        false
    };

    if !is_authority {
        return Vec::new();
    }

    let col = static_pos + 1; // 1-indexed at the `static` keyword.
    vec![(
        col,
        format!(
            "`static {}: {}` is an ambient-authority process-global — D21 \
             (K2 / ADR-0072) forbids module-level mutable/interior-mutable \
             statics that confer authority (handles, runtimes, senders, hooks). \
             K2 deleted the five such globals (ACTIVE_WALLET_RUNTIME, \
             GLOBAL_BROKER, GLOBAL_DRIVER, the bunker/NIP-55 HOOKs); a future PR \
             must not reintroduce one",
            name, ty
        ),
        "thread per-app state as an INSTANCE FIELD via an `Arc`-slot from the \
         composition root (the K2 `ActiveAccountSlot`/`BunkerHookSlot` pattern) \
         so two NmpApp instances never share authority; for a genuinely \
         read-once-config residual that is NOT a handle, annotate \
         `// doctrine-allow: D21 — <reason>`"
            .to_string(),
    )]
}

#[cfg(test)]
#[path = "d21/tests.rs"]
mod tests;
