use std::collections::BTreeSet;
use std::fs;

use crate::support::{crates_dir, evaluate, read, rel, Occurrence};

/// Rule E classification for a `crates/*` package per
/// `docs/architecture/crate-boundaries.md` §2.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CrateClass {
    /// A runtime crate in the numbered dependency model.
    Layer(u8),
    /// A non-runtime sidecar that is explicitly listed in the sidecar row.
    Exempt(&'static str),
}

/// Classify a workspace crate per `docs/architecture/crate-boundaries.md` §2.
/// Dependencies must flow from higher layers to lower layers; an edge to a
/// *higher* layer number is an upward inversion.
///
/// `nmp-nipNN` crates default to L4 (the reusable-protocol layer); the few
/// `*-types` NIP crates are L0 vocabulary and are matched explicitly first.
pub(crate) fn crate_class(name: &str) -> Option<CrateClass> {
    match name {
        // L0 — dependency-light vocabulary / interface types.
        "nmp-kinds" | "nmp-ownership" | "nmp-signer-iface" | "nmp-nip42-types"
        | "nmp-nip65-types" | "nmp-nip92-types" | "nmp-nip59" | "nmp-relay-url"
        | "nmp-nostr-id" => Some(CrateClass::Layer(0)),
        // L1 — storage, network transport, concrete signer transport.
        "nmp-store" | "nmp-nostr-lmdb" | "nmp-network" | "nmp-signers" | "nmp-sqlite-wasm" => {
            Some(CrateClass::Layer(1))
        }
        // L2 — routing and subscription planning.
        "nmp-router" | "nmp-planner" => Some(CrateClass::Layer(2)),
        // L3 — kernel substrate.
        "nmp-core" | "nmp-coverage-gate" => Some(CrateClass::Layer(3)),
        // L4 — reusable Nostr protocol/product modules (non-NIP members).
        "nmp-blossom"
        | "nmp-content"
        | "nmp-content-fixtures"
        | "nmp-feed"
        | "nmp-feed-session"
        | "nmp-read-session"
        | "nmp-threading"
        | "nmp-wot"
        | "nmp-marmot"
        | "nmp-intent"
        | "nmp-note-feed"
        | "nmp-nwc"
        | "nmp-reactions"
        | "nmp-replies"
        | "nmp-reposts"
        | "nmp-zaps" => Some(CrateClass::Layer(4)),
        // L5 — app/runtime composition floor.
        "nmp-substrate" => Some(CrateClass::Layer(5)),
        // L6 — platform runtimes / bindings.
        "nmp-native-runtime" | "nmp-uniffi" | "nmp-uniffi-support" | "nmp-browser-runtime" => {
            Some(CrateClass::Layer(6))
        }
        // Sidecars — tooling, tests, conformance vehicles, and private DX
        // proofs. These are classified explicitly so a new crate cannot slip
        // out of Rule E by omission.
        "nmp-cli" => Some(CrateClass::Exempt("developer CLI sidecar")),
        "nmp-codegen" => Some(CrateClass::Exempt("code generation sidecar")),
        "nmp-component-registry" => Some(CrateClass::Exempt(
            "component registry manifest/export sidecar",
        )),
        "nmp-testing" => Some(CrateClass::Exempt("test and benchmark sidecar")),
        "nmp-browser-runtime-conformance" => {
            Some(CrateClass::Exempt("browser runtime conformance sidecar"))
        }
        "nmp-sqlite-wasm-conformance" => {
            Some(CrateClass::Exempt("sqlite wasm conformance sidecar"))
        }
        "nmp-example-login-timeline" => Some(CrateClass::Exempt("private DX proof sidecar")),
        // Every other `nmp-nipNN` is an L4 reusable protocol crate.
        _ if name.starts_with("nmp-nip") && !name.ends_with("-types") => Some(CrateClass::Layer(4)),
        _ => None,
    }
}

/// Numbered layer for a runtime crate, or `None` for explicitly exempt
/// sidecars and unknown crates.
pub(crate) fn crate_layer(name: &str) -> Option<u8> {
    match crate_class(name) {
        Some(CrateClass::Layer(layer)) => Some(layer),
        _ => None,
    }
}

/// Classify a `from -> to` edge as upward, returning `(from_layer, to_layer)`
/// when both crates are in the documented layer model and `to` sits in a
/// strictly higher layer. Returns `None` for downward/same-layer edges and for
/// edges touching an unmapped crate.
pub(crate) fn upward_edge(from: &str, to: &str) -> Option<(u8, u8)> {
    match (crate_layer(from), crate_layer(to)) {
        (Some(a), Some(b)) if b > a => Some((a, b)),
        _ => None,
    }
}

#[test]
fn rule_e_classifies_every_crates_manifest() {
    let crates = crates_dir();
    let mut classified = 0usize;
    let mut unmapped = Vec::new();

    for entry in fs::read_dir(&crates).expect("read crates dir") {
        let dir = entry.expect("dir entry").path();
        if !dir.is_dir() {
            continue;
        }
        let name = match dir.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };
        if !dir.join("Cargo.toml").is_file() {
            continue;
        }
        match crate_class(&name) {
            Some(_) => classified += 1,
            None => unmapped.push(name),
        }
    }

    assert!(
        classified > 50,
        "Rule E classified only {classified} crates — coverage would be vacuous"
    );
    assert!(
        unmapped.is_empty(),
        "Rule E crate map must classify every crates/* Cargo.toml as a layer or \
         explicit sidecar exemption; unmapped: {}",
        unmapped.join(", ")
    );
}

/// Dependency name declared on a single `Cargo.toml` dependency line, honouring
/// the renamed-dependency form (`alias = { package = "real-name", ... }`).
pub(crate) fn dep_name(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('[') {
        return None;
    }
    let key: String = trimmed
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    if key.is_empty() {
        return None;
    }
    // The leading token must be a real key (followed by `=` or `.`), not prose.
    let after = trimmed[key.len()..].trim_start();
    if !(after.starts_with('=') || after.starts_with('.')) {
        return None;
    }
    // Renamed dependency: the real crate is the `package = "..."` value.
    if let Some(idx) = trimmed.find("package") {
        let rest = &trimmed[idx + "package".len()..];
        if let Some(q1) = rest.find('"') {
            let start = q1 + 1;
            if let Some(q2) = rest[start..].find('"') {
                return Some(rest[start..start + q2].to_string());
            }
        }
    }
    Some(key)
}

/// Parse a `Cargo.toml`, returning the names declared under any
/// `[dependencies]`, `[build-dependencies]`, or `[target.*.(build-)dependencies]`
/// section. `[dev-dependencies]` (and the `[target.*.dev-dependencies]` form)
/// are excluded — test-only edges do not constrain the runtime layer graph.
pub(crate) fn manifest_runtime_deps(toml: &str) -> Vec<String> {
    let mut deps = Vec::new();
    let mut collecting = false;
    for raw in toml.lines() {
        let line = raw.trim();
        if let Some(header) = line.strip_prefix('[') {
            let header = header.split(']').next().unwrap_or("");
            let is_dev = header.ends_with("dev-dependencies");
            collecting = !is_dev
                && (header == "dependencies"
                    || header == "build-dependencies"
                    || header.ends_with(".dependencies")
                    || header.ends_with(".build-dependencies"));
            continue;
        }
        if collecting {
            if let Some(name) = dep_name(line) {
                deps.push(name);
            }
        }
    }
    deps
}

/// Fine-grained baseline (tracked debt): `(manifest, dependency)`. Each entry
/// is one currently-existing upward Cargo edge. The owning fix PR removes its
/// line when it deletes the edge. Do NOT add new entries — a new upward edge
/// must be re-shaped (move the shared type down a layer, or invert through a
/// trait the lower crate owns), not baselined.
///
/// NOTE (#2526): when the NIP-19 extraction lands it introduces a
/// `nmp-core` (L3) -> `nmp-nip19` (L4) upward edge. That edge must be added
/// here under #2515 — never left to slip in silently — and #2526's follow-up
/// L0 rework then deletes the edge (and this line) again.
const RULE_E_BASELINE: &[(&str, &str)] = &[
    // crate-boundaries.md §4 — blessed dependency inversion. nmp-router (L2)
    // implements kernel traits owned by nmp-core (L3); the kernel only ever
    // sees `Arc<dyn OutboxRouter>` / `Arc<dyn MailboxCache>` injected at
    // composition, never a concrete router dependency. PERMANENT inversion
    // (stays baselined), not debt to retire.
    ("crates/nmp-router/Cargo.toml", "nmp-core"),
];

#[test]
fn rule_e_no_upward_cargo_edges() {
    let crates = crates_dir();
    let mut manifests = 0usize;
    let mut occs = Vec::new();

    for entry in fs::read_dir(&crates).expect("read crates dir") {
        let dir = entry.expect("dir entry").path();
        if !dir.is_dir() {
            continue;
        }
        let from = match dir.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };
        let manifest = dir.join("Cargo.toml");
        if !manifest.is_file() {
            continue;
        }
        manifests += 1;
        let toml = read(&manifest);
        let mut seen: BTreeSet<String> = BTreeSet::new();
        for to in manifest_runtime_deps(&toml) {
            if !seen.insert(to.clone()) {
                continue;
            }
            if let Some((lf, lt)) = upward_edge(&from, &to) {
                occs.push(Occurrence {
                    file: rel(&manifest),
                    key: to.clone(),
                    line: 0,
                    detail: format!(
                        "upward Cargo edge {from} (L{lf}) -> {to} (L{lt}): a lower-layer \
                         crate must not depend on a higher-layer crate"
                    ),
                });
            }
        }
    }

    assert!(
        manifests > 30,
        "Rule E scanned only {manifests} manifests — gate would be vacuous"
    );
    // Non-vacuous guard: the layer map must actually resolve the core crates.
    assert_eq!(
        crate_layer("nmp-core"),
        Some(3),
        "layer map must map nmp-core"
    );
    assert_eq!(
        crate_layer("nmp-router"),
        Some(2),
        "layer map must map nmp-router"
    );

    evaluate(
        "Rule E (upward-cargo-edge)",
        "a lower-layer crate must not depend on a higher-layer crate \
         (crate-boundaries.md §2). Move the shared type to the lower layer, or invert \
         through a trait the lower crate owns and inject the concrete impl at composition.",
        RULE_E_BASELINE,
        &occs,
    );
}
