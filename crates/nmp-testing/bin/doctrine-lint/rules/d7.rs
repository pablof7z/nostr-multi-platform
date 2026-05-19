//! D7 — capabilities report; never decide policy.
//!
//! `CapabilityModule` and any related traits in `substrate/capability.rs`
//! define the surface platform bridges (Keychain, NIP-46 bunker, BGTask
//! scheduler, FilePicker, etc.) expose to the kernel. Per D7 these bridges
//! *report* — they emit raw OS-level results back. They never *decide*:
//! never retry, never fall back to another path, never select a relay, never
//! route, never choose between options.
//!
//! Concretely: methods on capability traits whose **name** contains a
//! policy-decision verb are a D7 violation. The verb in a *doc-comment*
//! ("This capability does NOT retry — see policy in `nmp-core`") is fine.
//!
//! ## Banned verbs (in method names on traits inside `capability.rs`)
//!
//! - `retry`, `fallback`, `select`, `choose`, `route_to`, `decide`,
//!   `dispatch_to`, `resolve_to`
//!
//! ## Scope
//!
//! This rule scans only `crates/nmp-core/src/substrate/capability.rs`. Other
//! files in `substrate/` are out of scope. The rule fires when a line both
//! (a) declares a function (`fn name(...)`) AND (b) the function name
//! contains a banned verb.

use std::path::Path;

pub const ID: &str = "D7";

const TARGET_FILE_SUFFIX: &str = "substrate/capability.rs";

const BANNED_VERBS: &[&str] = &[
    "retry", "fallback", "select", "choose", "route_to", "decide", "dispatch_to", "resolve_to",
];

pub fn file_in_scope(path: &Path) -> bool {
    let s = path.to_string_lossy().replace('\\', "/");
    s.ends_with(TARGET_FILE_SUFFIX)
}

pub fn check(line: &str, is_comment: bool) -> Vec<(usize, String, String)> {
    if is_comment {
        return Vec::new();
    }
    // Look for `fn <name>(` patterns. `<name>` is the next identifier.
    let Some(fn_pos) = find_fn_decl(line) else {
        return Vec::new();
    };
    let after_fn = &line[fn_pos + 3..]; // skip "fn "
    let name_end = after_fn
        .find(|c: char| !(c.is_alphanumeric() || c == '_'))
        .unwrap_or(after_fn.len());
    let name = &after_fn[..name_end];

    let mut hits = Vec::new();
    for verb in BANNED_VERBS {
        if name.contains(verb) {
            hits.push((
                fn_pos + 1,
                format!(
                    "method `{}` on a capability trait names a policy-decision verb `{}` — D7 forbids capabilities deciding policy",
                    name, verb
                ),
                format!(
                    "rename to a *reporting* verb (`emit_*`, `report_*`, `query_*`, `observe_*`) and move the `{}` logic into `nmp-core`",
                    verb
                ),
            ));
        }
    }
    hits
}

fn find_fn_decl(line: &str) -> Option<usize> {
    // Match `fn ` preceded by start-of-line whitespace OR by a visibility/
    // async/extern keyword + space. We accept any position where the bytes
    // ` fn ` appear (with a leading space) OR the line starts with `fn `.
    let bytes = line.as_bytes();
    if bytes.starts_with(b"fn ") {
        return Some(0);
    }
    line.find(" fn ").map(|i| i + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_retry_method_name() {
        let hits = check("    fn retry_authentication(&self);", false);
        assert_eq!(hits.len(), 1);
        assert!(hits[0].1.contains("retry"));
    }

    #[test]
    fn flags_select_in_name() {
        let hits = check("    fn select_relay(&self) -> RelayUrl;", false);
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn ignores_verb_in_doc_comment() {
        let hits = check("/// This capability does NOT retry — that's policy.", true);
        assert!(hits.is_empty());
    }

    #[test]
    fn ignores_reporting_verb() {
        let hits = check("    fn report_failure(&self) -> Envelope;", false);
        assert!(hits.is_empty());
    }

    #[test]
    fn ignores_no_fn_decl() {
        let hits = check("    self.dispatch_to_relay(url);", false);
        assert!(hits.is_empty(), "method call ≠ method decl; only decl names matter");
    }

    #[test]
    fn flags_pub_fn() {
        let hits = check("    pub fn choose_strategy(&self);", false);
        assert_eq!(hits.len(), 1);
    }
}
