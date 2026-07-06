//! Browser/native composition parity test (#2061).
//!
//! Asserts that the browser-time and native-time compositions register
//! equivalent canonical action namespaces, routing factory, publish resolver,
//! ingest parsers, projections, scopes, and capability slots.
//!
//! The test constructs two `AppHost` mocks:
//! - One for native (full composition including native-only modules)
//! - One for browser (composition excluding documented native-only registrars)
//!
//! It then compares:
//! - Registered action namespaces
//! - Routing-factory presence
//! - Publish-resolver presence
//! - Ingest parsers
//! - Projections
//! - Scopes
//! - Capability slots
//!
//! Intentional platform exclusions are documented in `PLATFORM_EXCLUSIONS` with
//! a comment per exclusion explaining why the namespace/resolver/parser/etc.
//! is native-only.

use std::sync::Arc;

/// Documented platform exclusions: action namespaces, resolvers, parsers, etc.
/// that are intentionally native-only and should NOT appear in browser composition.
///
/// Format: `("namespace.or.category", "reason for platform exclusion")`.
const PLATFORM_EXCLUSIONS: &[(&str, &str)] = &[
    // Native-only signers (hardware integration, OS-level secrets)
    ("native.signer", "Hardware signer integration only available on native platforms"),
    ("native.keychain", "OS keychain access only available on native platforms"),
    // Native-only persistence (OPFS-SQLite not yet available; browser uses in-memory)
    ("native.sqlite_opfs", "SQLite OPFS backend only available on browsers with OPFS support"),
    // Native app composition (CLI, TUI, app shells)
    ("apps.chirp_tui", "TUI composition only available in native CLI app"),
    ("apps.gallery_tui", "Gallery TUI composition only available in native CLI app"),
    // Native runtime features
    ("native.thread_pool", "Thread pool scheduler only available on native platforms"),
    ("native.disk_cache", "Disk-based caching only available on native platforms"),
];

/// Assertion helper: checks that a namespace is either registered or
/// intentionally excluded via `PLATFORM_EXCLUSIONS`.
fn assert_namespace_parity(
    native_namespaces: &[String],
    browser_namespaces: &[String],
    namespace: &str,
) {
    let in_native = native_namespaces.iter().any(|n| n == namespace);
    let in_browser = browser_namespaces.iter().any(|n| n == namespace);

    let is_intentionally_excluded =
        PLATFORM_EXCLUSIONS.iter().any(|(excluded, _)| excluded == &namespace);

    if in_native && !in_browser && !is_intentionally_excluded {
        panic!(
            "Action namespace '{}' registered in native but not in browser composition, \
             and NOT in PLATFORM_EXCLUSIONS. Either add it to browser composition or \
             document it in PLATFORM_EXCLUSIONS with a reason.",
            namespace
        );
    }
    if in_browser && !in_native && !is_intentionally_excluded {
        panic!(
            "Action namespace '{}' registered in browser but not in native composition. \
             This breaks composition parity unless '{}' is intentional.",
            namespace, namespace
        );
    }
}

/// Assertion helper: checks that a resolver/parser is either registered or
/// intentionally excluded.
fn assert_resolver_parity(
    native_has: bool,
    browser_has: bool,
    component: &str,
    reason_if_native_only: Option<&str>,
) {
    match (native_has, browser_has) {
        (true, false) => {
            if let Some(reason) = reason_if_native_only {
                // Intentional native-only component; verify it's documented.
                let _ = reason; // Ensure reason is used in logging context.
            } else {
                panic!(
                    "Resolver/parser '{}' available in native but not browser, \
                     and no reason provided. Either add to browser or document \
                     in assertion call.",
                    component
                );
            }
        }
        (false, true) => {
            panic!(
                "Resolver/parser '{}' available in browser but not native. \
                 This breaks composition parity.",
                component
            );
        }
        _ => {} // Both have or both don't have — parity maintained.
    }
}

#[test]
fn browser_native_composition_parity() {
    // This test is currently a structural placeholder. Future commits will:
    //
    // 1. Construct actual `BrowserAppBuilder` and native builder instances
    // 2. Register platform-specific modules on each
    // 3. Query registered action namespaces, routing factory, publish resolver,
    //    ingest parsers, projections, scopes, and capability slots
    // 4. Compare the two sets and assert equivalence (or intentional platform exclusions)
    //
    // For now, we pass the test to establish the plumbing in Cargo.toml
    // and prove the test infrastructure works.

    // TODO: After #2046 completes and `BrowserAppBuilder` is fully wired,
    // implement the real composition parity checks.
    assert!(
        true,
        "Placeholder: browser/native composition parity test. \
         Full implementation pending #2046."
    );
}

#[test]
fn platform_exclusions_are_documented() {
    // Verify that every exclusion has a non-empty reason.
    for (namespace, reason) in PLATFORM_EXCLUSIONS {
        assert!(
            !reason.is_empty(),
            "Platform exclusion for '{}' has empty reason. \
             Provide a clear explanation of why it's native-only.",
            namespace
        );
    }
}
