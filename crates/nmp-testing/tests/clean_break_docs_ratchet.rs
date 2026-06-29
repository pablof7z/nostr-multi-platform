//! Clean-break docs vocabulary ratchet (#2342).
//!
//! Production docs must not grow new guidance that teaches old app-facing
//! architecture as the normal way to build NMP apps. Historical ADRs, wiki,
//! research, perf reports, and retired documents are intentionally out of scope.

use std::path::{Path, PathBuf};
use std::process::Command;

type Pattern = (&'static str, &'static str, &'static str);
type Allow = (&'static str, &'static str, &'static str, &'static str);

const PATTERNS: &[Pattern] = &[
    (
        "register_defaults()",
        "register_defaults",
        "production docs should teach explicit feature composition; presets \
         must be tutorial/test/migration compatibility with owner and deletion trigger",
    ),
    (
        "nmp_app_open_interest",
        "raw_interest_ffi",
        "product reads should use typed sessions/helpers; raw interest is \
         low-level acquisition machinery",
    ),
    (
        "open_interest",
        "raw_interest",
        "product reads should use typed sessions/helpers; raw interest is \
         low-level acquisition machinery",
    ),
    (
        "ObservedProjectionSink",
        "observed_projection",
        "observed projection sinks are internal read-session machinery unless \
         explicitly documented as substrate",
    ),
    (
        "ObservedProjection",
        "observed_projection",
        "observed projections are internal read-session machinery unless \
         explicitly documented as substrate",
    ),
    (
        "ReducedSource",
        "reduced_source",
        "ReducedSource is internal source-reconciliation/compiler vocabulary, \
         not app-facing setup",
    ),
    (
        "nmp.feed.home",
        "home_feed_singleton",
        "`nmp.feed.home` may be a projection key, not a special singleton app \
         architecture",
    ),
    (
        "anonymous explicit relay",
        "anonymous_explicit_relay",
        "manual relay routes must carry typed route provenance",
    ),
    (
        "PublishRaw",
        "write_raw_publish",
        "starter/app write docs should teach typed product or protocol builders; raw publish is internal/protocol/import machinery",
    ),
    (
        "publishRaw",
        "write_raw_publish_method",
        "starter/app write docs should not point shells at the raw publish builder",
    ),
    (
        "pre-signed",
        "write_presigned_publish",
        "pre-signed/verbatim publish must be documented only as imported/protocol-owned escape machinery",
    ),
    (
        "manual_override",
        "write_manual_override_route",
        "manual route tokens must not appear as anonymous app-facing defaults",
    ),
    (
        "signer_pubkey",
        "write_raw_signer_pubkey",
        "public write docs should teach typed signer provenance, not raw optional signer_pubkey policy",
    ),
    (
        "signerPubkey",
        "write_raw_signer_pubkey",
        "public write docs should teach typed signer provenance, not raw optional signerPubkey policy",
    ),
];

// path, token, required line text (empty means whole file), reason.
// New hits must be corrected in place or added here with a concrete reason.
#[rustfmt::skip]
const ALLOWLIST: &[Allow] = &[
    ("docs/architecture/crate-boundaries.md", "ObservedProjectionSink", "", "internal substrate plumbing"),
    ("docs/architecture/crate-boundaries.md", "ReducedSource", "", "internal compiler ownership"),
    ("docs/architecture/high-level-app-architecture.md", "open_interest", "Internally, this may use `open_interest`", "internal machinery"),
    ("docs/architecture/high-level-app-architecture.md", "ReducedSource", "`ReducedSource`-style dynamic source reconciliation", "internal machinery"),
    ("docs/architecture/high-level-app-architecture.md", "open_interest", "app-facing raw `open_interest`", "deletion-target list"),
    ("docs/architecture/high-level-app-architecture.md", "ObservedProjection", "app-facing `ObservedProjection`", "deletion-target list"),
    ("docs/architecture/high-level-app-architecture.md", "ObservedProjectionSink", "or `ObservedProjectionSink` recipes", "deletion-target list"),
    ("docs/architecture/high-level-app-architecture.md", "ReducedSource", "public `ReducedSource` vocabulary", "deletion-target list"),
    ("docs/architecture/high-level-app-architecture.md", "nmp.feed.home", "special `nmp.feed.home` singleton wiring", "deletion-target list"),
    ("docs/architecture/high-level-app-architecture.md", "register_defaults()", "production `register_defaults()` as the normal app root", "deletion-target list"),
    ("docs/architecture/high-level-app-architecture.md", "anonymous explicit relay", "anonymous explicit relay lists as product publish state", "deletion-target list"),
    ("docs/architecture/external-consumers.md", "open_interest", "Raw `open_interest` is low-level internal acquisition machinery", "external-consumer non-public inventory"),
    ("docs/product-spec/api-surface.md", "nmp_app_open_interest", "The public raw-interest C ABI (`nmp_app_open_interest`", "deletion ledger"),
    ("docs/product-spec/api-surface.md", "nmp_app_open_interest", "old public doors deleted or privatized: `nmp_app_open_interest`", "deletion ledger"),
    ("docs/product-spec/api-surface.md", "open_interest", "Raw `open_interest` remains low-level", "internal machinery"),
    ("docs/product-spec/api-surface.md", "open_interest", "`open_interest` machinery remains for substrate/protocol use", "deletion ledger"),
    ("docs/product-spec/subsystems.md", "ReducedSource", "ReducedSources and dependent interests compile", "internal subsystem compiler vocabulary"),
    ("docs/product-spec/subsystems.md", "anonymous explicit relay", "Anonymous explicit relay lists are not product state", "negative rule"),
    ("docs/product-spec/doctrine.md", "ReducedSource", "from a ReducedSource such as the active account", "doctrine example names internal compiler"),
    ("docs/ffi-surface.md", "ReducedSource", "active-follows is one ReducedSource instance", "FFI migration note"),
    ("docs/recipes/app-shapes.md", "nmp.feed.home", "\"projection\": \"nmp.feed.home\"", "projection-key example"),
    ("docs/recipes/app-shapes.md", "open_interest", "Use low-level `open_interest` only for static non-feed", "demotes raw interest"),
    ("docs/recipes/app-shapes.md", "ObservedProjection", "`ObservedProjection` as app-facing product APIs", "negative rule"),
    ("docs/builder-guide/01-what-nmp-is.md", "ReducedSource", "", "comparison table names internal primitive"),
    ("docs/builder-guide/05a-substrate-traits.md", "ObservedProjectionSink", "", "substrate trait reference"),
    ("docs/builder-guide/05a-substrate-traits.md", "ObservedProjection", "", "substrate trait reference"),
    ("docs/builder-guide/05b-substrate-traits.md", "ObservedProjectionSink", "", "substrate walkthrough marks sink internal"),
    ("docs/builder-guide/05b-substrate-traits.md", "ObservedProjection", "", "substrate walkthrough marks sink internal"),
    ("docs/builder-guide/06-reactivity-contract.md", "ObservedProjectionSink", "", "reactivity contract internal delivery"),
    ("docs/builder-guide/06-reactivity-contract.md", "ReducedSource", "", "reactivity contract internal source owner"),
    ("docs/builder-guide/07-subscription-planner.md", "ReducedSource", "", "subscription planner reference"),
    ("docs/builder-guide/10-outbox-routing.md", "ReducedSource", "", "outbox routing reference"),
    ("docs/builder-guide/11-sessions-signers.md", "ReducedSource", "", "sessions/signers reference"),
    ("docs/builder-guide/15-codegen-and-ffi.md", "register_defaults()", "", "tutorial/test/migration preset reference"),
    ("docs/builder-guide/15-codegen-and-ffi.md", "nmp.feed.home", "", "projection key example"),
    ("docs/builder-guide/18-testing.md", "ReducedSource", "", "testing guide names internal behavior"),
    ("docs/builder-guide/19a-walkthrough-microblog.md", "register_defaults()", "", "walkthrough rejects hidden production presets"),
    ("docs/builder-guide/19a-walkthrough-microblog.md", "ObservedProjectionSink", "", "walkthrough identifies internal sink"),
    ("docs/builder-guide/19b-walkthrough-microblog.md", "ObservedProjectionSink", "internal session-executor", "walkthrough status table"),
    ("docs/builder-guide/20-new-protocol-module.md", "ObservedProjectionSink", "", "protocol-module internal delivery"),
    ("docs/builder-guide/20-new-protocol-module.md", "ObservedProjection", "", "protocol-module internal delivery"),
    ("docs/builder-guide/21-framework-magic.md", "ReducedSource", "", "framework-magic reference"),
    ("docs/builder-guide/23-glossary.md", "ObservedProjectionSink", "", "glossary internal vocabulary"),
    ("docs/builder-guide/23-glossary.md", "ReducedSource", "source-reconciliation", "glossary internal vocabulary"),
    ("docs/builder-guide/24-reference-cards.md", "ObservedProjectionSink", "", "reference card contrast"),
    ("docs/builder-guide/25-migration-from-ndk-applesauce.md", "ReducedSource", "internal compiler vocabulary", "migration mapping"),
    ("docs/builder-guide/26-faq-troubleshooting.md", "nmp.feed.home", "", "projection key sidecar"),
    ("docs/builder-guide/28-action-triggered-subscriptions.md", "ObservedProjectionSink", "are implementation", "implementation-detail guide"),
    ("docs/builder-guide/28-action-triggered-subscriptions.md", "ReducedSource", "materialize child interests", "internal Rust owner"),
    // public-surface disposition table (#2378) — four additive entries scoped to one doc;
    // appended last so #2366's allowlist-shrink rebases cleanly.
    ("docs/architecture/high-level-app-architecture.md", "open_interest", "Internal acquisition machinery behind typed read sessions (ADR-0070)", "public-surface disposition table (#2378)"),
    ("docs/architecture/high-level-app-architecture.md", "ObservedProjection", "Internal event-delivery and replay machinery behind typed read sessions (ADR-0070)", "public-surface disposition table (#2378)"),
    ("docs/architecture/high-level-app-architecture.md", "ReducedSource", "Internal dynamic source reconciliation behind a session (ADR-0070)", "public-surface disposition table (#2378)"),
    ("docs/architecture/high-level-app-architecture.md", "nmp.feed.home", "A projection key for the typed", "public-surface disposition table (#2378)"),
    // write-lane cleanup (#2401): allowed only where the public doc explicitly
    // demotes the old vocabulary to internal/protocol/import machinery.
    ("docs/architecture/high-level-app-architecture.md", "pre-signed", "Imported/pre-signed events stay imported/manual", "protocol/import escape disposition (#2401)"),
    ("docs/architecture/high-level-app-architecture.md", "pre-signed", "imported/pre-signed events stay imported/manual", "protocol/import escape example (#2401)"),
    ("docs/wasm-surface.md", "signer_pubkey", "`invalid_signer_pubkey`", "stable degraded-mode error prefix, not write API guidance (#2401)"),
];

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repo root is two levels above crates/nmp-testing")
        .to_path_buf()
}

fn git_tracked_markdown(root: &Path) -> Vec<PathBuf> {
    let output = Command::new("git")
        .arg("ls-files")
        .arg("--")
        .arg("README.md")
        .arg("docs")
        .current_dir(root)
        .output()
        .expect("git ls-files must run");
    assert!(
        output.status.success(),
        "git ls-files failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| line.ends_with(".md"))
        .map(|line| root.join(line))
        .filter(|path| production_doc_in_scope(root, path))
        .collect()
}

fn production_doc_in_scope(root: &Path, path: &Path) -> bool {
    let rel = rel_path(root, path);
    if rel == "README.md" {
        return true;
    }
    rel.starts_with("docs/")
        && ![
            "docs/decisions/",
            "docs/design/",
            "docs/perf/",
            "docs/research/",
            "docs/retired/",
            "docs/testing/",
            "docs/wiki/",
        ]
        .iter()
        .any(|prefix| rel.starts_with(prefix))
}

fn rel_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn allowed(rel: &str, token: &str, line: &str) -> Option<&'static str> {
    ALLOWLIST
        .iter()
        .find_map(|(path, allow_token, contains, reason)| {
            (*path == rel
                && *allow_token == token
                && (contains.is_empty() || line.contains(contains)))
            .then_some(*reason)
        })
}

fn token_matches(line: &str, token: &str) -> bool {
    match token {
        "ObservedProjection" | "open_interest" => contains_identifier(line, token),
        _ => line.contains(token),
    }
}

fn contains_identifier(line: &str, token: &str) -> bool {
    let mut start = 0;
    while let Some(offset) = line[start..].find(token) {
        let idx = start + offset;
        let before = line[..idx].chars().next_back();
        let after = line[idx + token.len()..].chars().next();
        if !is_ident_char(before) && !is_ident_char(after) {
            return true;
        }
        start = idx + token.len();
    }
    false
}

fn is_ident_char(ch: Option<char>) -> bool {
    ch.is_some_and(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

#[test]
fn production_docs_do_not_grow_old_app_facing_architecture_vocabulary() {
    let root = repo_root();
    let files = git_tracked_markdown(&root);
    assert!(!files.is_empty(), "docs ratchet must scan markdown files");

    let mut violations = Vec::new();
    for file in files {
        let rel = rel_path(&root, &file);
        let text = std::fs::read_to_string(&file)
            .unwrap_or_else(|err| panic!("read {}: {err}", file.display()));
        for (line_idx, line) in text.lines().enumerate() {
            for &(token, label, guidance) in PATTERNS {
                if token_matches(line, token) && allowed(&rel, token, line).is_none() {
                    violations.push(format!(
                        "{}:{}: error[clean_break_docs:{}]: `{}` - {}\n    {}",
                        rel,
                        line_idx + 1,
                        label,
                        token,
                        guidance,
                        line.trim()
                    ));
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "production docs contain old app-facing architecture vocabulary not \
         covered by the clean-break ratchet allowlist. Point production docs at \
         ADR-0069..0073 and docs/architecture/high-level-app-architecture.md; \
         historical/internal mentions need a concrete allowlist reason.\n{}",
        violations.join("\n")
    );
}
