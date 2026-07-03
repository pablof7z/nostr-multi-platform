//! Trellis public-surface leak ratchets (#2634).
//!
//! Trellis may be used by private NMP internals, validation tests, and the
//! dev-build-only `nmp-devtools` diagnostic surface. Public app/native/web
//! surfaces must continue to expose NMP typed sessions, projections, actions,
//! and outputs rather than raw Trellis graph primitives.

use super::trellis_public_api_support::{
    collect_files, is_diagnostic_trellis_surface, normalize_path, public_surface_files,
    scan_files_for_trellis_tokens, strip_line_comments, trellis_token_hits, SurfaceMode,
    ALLOWED_PRIVATE_TRELLIS_PATHS, BUILDER_SURFACE_ROOTS, RUST_PUBLIC_SURFACE_ROOTS,
};
use super::workspace_root;

#[test]
fn trellis_primitives_do_not_leak_to_native_web_or_app_facing_rust_surfaces() {
    let root = workspace_root();
    let mut files = Vec::new();
    for rel in RUST_PUBLIC_SURFACE_ROOTS {
        collect_files(&root.join(rel), &mut files, &["rs", "udl"]);
    }

    let violations = scan_files_for_trellis_tokens(&root, &files, SurfaceMode::Rust);
    assert!(
        violations.is_empty(),
        "raw Trellis primitives must stay out of native/web/app-facing Rust APIs; \
         expose NMP typed session/action/projection surfaces instead:\n{}",
        violations.join("\n")
    );
}

#[test]
fn builder_docs_and_examples_do_not_teach_raw_trellis_graph_assembly() {
    let root = workspace_root();
    let mut files = Vec::new();
    for rel in BUILDER_SURFACE_ROOTS {
        collect_files(&root.join(rel), &mut files, &["md", "rs", "ts", "tsx"]);
    }

    let violations = scan_files_for_trellis_tokens(&root, &files, SurfaceMode::DocsAndExamples);
    assert!(
        violations.is_empty(),
        "builder-facing docs/examples must teach NMP typed sessions, not raw \
         Trellis graph/resource/output assembly:\n{}",
        violations.join("\n")
    );
}

#[test]
fn private_trellis_internals_are_not_part_of_the_public_surface_scan() {
    let root = workspace_root();
    let scanned = public_surface_files(&root);
    for rel in ALLOWED_PRIVATE_TRELLIS_PATHS {
        let path = normalize_path(&root.join(rel));
        assert!(
            !scanned.contains(&path),
            "`{rel}` is a private/internal Trellis owner and must not be \
             pulled into the public-surface leak scan"
        );
    }
}

#[test]
fn diagnostic_trellis_surface_is_narrowly_exempt() {
    let root = workspace_root();
    assert!(
        is_diagnostic_trellis_surface(&root, &root.join("crates/nmp-devtools/src/lib.rs")),
        "`nmp-devtools` is the only Trellis-visible diagnostic surface"
    );
    assert!(
        !is_diagnostic_trellis_surface(
            &root,
            &root.join("crates/nmp-native-runtime/src/devtools.rs")
        ),
        "runtime/native app surfaces must not inherit the diagnostic exemption"
    );
    assert!(
        !is_diagnostic_trellis_surface(&root, &root.join("docs/builder-guide/devtools.md")),
        "builder-facing docs must continue to hide raw Trellis vocabulary"
    );
}

#[test]
fn trellis_token_matcher_flags_raw_primitives_without_matching_longer_names() {
    let hits = trellis_token_hits(
        "pub fn leak() -> trellis_core::Graph<ResourceKey, OutputFrame> { todo!() }",
    );
    assert!(hits.contains(&"trellis_core"));
    assert!(hits.contains(&"Graph"));
    assert!(hits.contains(&"ResourceKey"));
    assert!(hits.contains(&"OutputFrame"));

    let longer_names = trellis_token_hits("SocialGraph ResourceKeyed OutputFrameKindness");
    assert!(
        longer_names.is_empty(),
        "Trellis primitive matching should require identifier boundaries"
    );
}

#[test]
fn rust_surface_scanner_ignores_commented_trellis_mentions() {
    let mut in_block_comment = false;
    let live = strip_line_comments(
        "let visible = 1; // trellis_core::Graph<ResourceKey>",
        &mut in_block_comment,
    );
    assert!(trellis_token_hits(&live).is_empty());

    let mut in_block_comment = false;
    let live = strip_line_comments("let visible = /* Graph */ 1;", &mut in_block_comment);
    assert!(trellis_token_hits(&live).is_empty());
}
