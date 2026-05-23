use crate::{app_crate_name, rust_crate_name, variant_name, AppManifest};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GenerationReport {
    pub app_name: String,
    pub crate_name: String,
    pub files: Vec<PathBuf>,
}

#[must_use]
pub fn generate_modules(manifest_path: &Path, out_dir: &Path) -> Result<GenerationReport, String> {
    let manifest = AppManifest::read(manifest_path)?;
    if out_dir.exists() {
        fs::remove_dir_all(out_dir).map_err(|error| error.to_string())?;
    }
    fs::create_dir_all(out_dir.join("src")).map_err(|error| error.to_string())?;

    let files = vec![
        ("Cargo.toml", cargo_toml(&manifest)),
        ("src/lib.rs", lib_rs()),
        ("src/action.rs", action_rs(&manifest)),
        ("src/update.rs", update_rs(&manifest)),
        ("src/envelope.rs", envelope_rs()),
        ("src/view_spec.rs", view_spec_rs(&manifest)),
        ("src/capability.rs", capability_rs(&manifest)),
        ("src/domain.rs", domain_rs(&manifest)),
        ("src/ffi.rs", ffi_rs(&manifest)),
    ];

    let mut written = Vec::new();
    for (relative, content) in files {
        let path = out_dir.join(relative);
        fs::write(&path, content).map_err(|error| error.to_string())?;
        written.push(PathBuf::from(relative));
    }

    Ok(GenerationReport {
        app_name: manifest.name.clone(),
        crate_name: app_crate_name(&manifest.name),
        files: written,
    })
}

fn cargo_toml(manifest: &AppManifest) -> String {
    // `nmp_app_new`, `nmp_app_free`, `nmp_app_dispatch_action` are re-exported
    // under the default `native` feature since PR #356 — no explicit feature
    // flag is needed for the generated app crate's dependency.
    let mut out = format!(
        "[package]\nname = \"{}\"\nversion.workspace = true\nedition.workspace = true\nlicense.workspace = true\n\n[dependencies]\nnmp-core = {{ path = \"../../../crates/nmp-core\" }}\nserde = {{ version = \"1.0\", features = [\"derive\"] }}\nserde_json = \"1.0\"\n",
        app_crate_name(&manifest.name)
    );
    for module in manifest.ordered_modules() {
        out.push_str(&format!(
            "{} = {{ package = \"{}\", path = \"../../../crates/{}\" }}\n",
            rust_crate_name(&module),
            module,
            module
        ));
    }
    out
}

fn lib_rs() -> String {
    [
        "pub mod action;",
        "pub mod capability;",
        "pub mod domain;",
        "pub mod envelope;",
        "pub mod ffi;",
        "pub mod update;",
        "pub mod view_spec;",
        "",
        "pub use action::AppAction;",
        "pub use envelope::UpdateEnvelope;",
        "pub use ffi::FfiApp;",
        "pub use update::AppUpdate;",
        "pub use view_spec::ViewSpec;",
        "",
    ]
    .join("\n")
}

fn action_rs(manifest: &AppManifest) -> String {
    enum_file(
        "AppAction",
        "nmp_core::KernelAction",
        manifest,
        "Action",
        "pub fn namespace(&self) -> &'static str",
    )
}

fn update_rs(manifest: &AppManifest) -> String {
    enum_file(
        "AppUpdate",
        "nmp_core::KernelUpdate",
        manifest,
        "Update",
        "pub fn namespace(&self) -> &'static str",
    )
}

/// The canonical update-channel envelope, projected for the host crate.
///
/// Every frame on the single update channel is one tagged outer object —
/// `{"t":"snapshot","v":<snapshot>}` or `{"t":"panic","v":{"msg":<message>}}` —
/// so the host decodes exactly **one** discriminated type. This MUST stay
/// byte-identical to `nmp_core::UpdateEnvelope`'s serde contract (tag `t`,
/// content `v`, snake_case variants); see
/// `docs/design/0001-ffi-update-channel-envelope.md`.
///
/// The snapshot interior is intentionally opaque (`serde_json::Value`) — this
/// type models the discriminator, not the snapshot's ~30 internal fields.
///
/// The `Panic` arm (D7) is the actor-death signal: the kernel loop panicked
/// or exited and the host must surface a fatal error rather than keep sending
/// commands. It reuses `nmp_core::PanicFrame` so the host carrier matches the
/// kernel's exactly.
fn envelope_rs() -> String {
    [
        "use serde::{Deserialize, Serialize};",
        "",
        "#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]",
        "#[serde(tag = \"t\", content = \"v\", rename_all = \"snake_case\")]",
        "pub enum UpdateEnvelope {",
        "    /// A full snapshot — replace rendered state.",
        "    Snapshot(serde_json::Value),",
        "    /// Actor-thread death (D7) — terminal; surface a fatal error.",
        "    Panic(nmp_core::PanicFrame),",
        "}",
        "",
    ]
    .join("\n")
}

fn view_spec_rs(manifest: &AppManifest) -> String {
    enum_file(
        "ViewSpec",
        "nmp_core::KernelViewSpec",
        manifest,
        "ViewSpec",
        "pub fn namespace(&self) -> &'static str",
    )
}

/// Emit a projected enum (`AppAction`/`AppUpdate`/`ViewSpec`) over the kernel
/// plus one variant per manifest module.
///
/// Pre-wiring contract: each module variant is emitted as `<crate>::Action`,
/// `<crate>::Update`, or `<crate>::ViewSpec` (per `module_type`) — so a module
/// crate must export those exact names at its crate root for the generated
/// app crate to compile. `fixture-todo-core` honors this; the real NIP crates
/// (`nmp-nip01`, `nmp-nip22`, …) do not, so codegen has no live NIP-module
/// consumer. Conforming those crates, or declaring per-module type paths in
/// `nmp.toml`, is the open seam (NMP-145).
fn enum_file(
    enum_name: &str,
    kernel_type: &str,
    manifest: &AppManifest,
    module_type: &str,
    method_sig: &str,
) -> String {
    let mut out = format!(
        "#[derive(Clone, Debug, PartialEq)]\npub enum {enum_name} {{\n    Kernel({kernel_type}),\n"
    );
    for module in manifest.ordered_modules() {
        out.push_str(&format!(
            "    {}({}::{}),\n",
            variant_name(&module),
            rust_crate_name(&module),
            module_type
        ));
    }
    out.push_str("}\n\n");
    out.push_str(&format!("impl {enum_name} {{\n    {method_sig} {{\n"));
    out.push_str("        match self {\n            Self::Kernel(_) => \"kernel\",\n");
    for module in manifest.ordered_modules() {
        out.push_str(&format!(
            "            Self::{}(_) => \"{}\",\n",
            variant_name(&module),
            module
        ));
    }
    out.push_str("        }\n    }\n}\n");
    out
}

fn capability_rs(manifest: &AppManifest) -> String {
    const_list("CAPABILITY_MODULE_CRATES", manifest)
}

fn domain_rs(manifest: &AppManifest) -> String {
    const_list("DOMAIN_MODULE_CRATES", manifest)
}

fn const_list(name: &str, manifest: &AppManifest) -> String {
    let values = manifest
        .ordered_modules()
        .into_iter()
        .map(|module| format!("\"{}\"", module))
        .collect::<Vec<_>>()
        .join(", ");
    format!("pub const {name}: &[&str] = &[{values}];\n")
}

fn ffi_rs(manifest: &AppManifest) -> String {
    crate::ffi_gen::ffi_rs(manifest)
}

#[cfg(test)]
mod tests {
    //! Pure-string tests for the private code formatters. None of these touch
    //! disk: the formatters take an in-memory `AppManifest` and return the
    //! emitted source as a `String`. Disk-backed end-to-end coverage lives in
    //! `tests/determinism.rs` and `tests/ffi_dispatch.rs`.

    use super::*;
    use crate::manifest::ModuleSet;

    /// Build an `AppManifest` literal — no file, no parse.
    fn manifest(protocol: &[&str], app: &[&str]) -> AppManifest {
        AppManifest {
            name: "fixture".to_string(),
            display_name: "Fixture".to_string(),
            modules: ModuleSet {
                kernel: "nmp-core".to_string(),
                protocol: protocol.iter().map(ToString::to_string).collect(),
                app: app.iter().map(ToString::to_string).collect(),
            },
        }
    }

    #[test]
    fn enum_file_with_zero_modules_emits_only_the_kernel_variant() {
        // Edge case — an empty manifest. The enum carries exactly `Kernel(_)`,
        // no module variants, and the `namespace()` match has one non-default
        // arm. There must be NO trailing module arms.
        let out = enum_file(
            "AppAction",
            "nmp_core::KernelAction",
            &manifest(&[], &[]),
            "Action",
            "pub fn namespace(&self) -> &'static str",
        );
        assert!(out.contains("pub enum AppAction {"));
        assert!(out.contains("Kernel(nmp_core::KernelAction),"));
        assert_eq!(
            out.matches("Self::").count(),
            1,
            "zero-module enum has exactly one match arm (the kernel arm)"
        );
        assert!(out.contains("Self::Kernel(_) => \"kernel\","));
    }

    #[test]
    fn enum_file_with_one_module_emits_exactly_one_variant() {
        let out = enum_file(
            "AppAction",
            "nmp_core::KernelAction",
            &manifest(&[], &["fixture-todo-core"]),
            "Action",
            "pub fn namespace(&self) -> &'static str",
        );
        // The single module variant uses the UpperCamelCase variant name, the
        // underscored crate path, and the `Action` module type.
        assert!(out.contains("FixtureTodoCore(fixture_todo_core::Action),"));
        // Its namespace match arm reports the ORIGINAL (dash-form) module name.
        assert!(out.contains("Self::FixtureTodoCore(_) => \"fixture-todo-core\","));
    }

    #[test]
    fn enum_file_with_n_modules_keeps_protocol_before_app_order() {
        // Multi-module: variant order must follow `ordered_modules()` —
        // protocol modules first, then app modules. This is what makes the
        // generated enum's discriminants stable across builds.
        let out = enum_file(
            "ViewSpec",
            "nmp_core::KernelViewSpec",
            &manifest(&["nmp-nip01", "nmp-nip22"], &["fixture-todo-core"]),
            "ViewSpec",
            "pub fn namespace(&self) -> &'static str",
        );
        let nip01 = out.find("NmpNip01(").expect("nip01 variant present");
        let nip23 = out.find("NmpNip22(").expect("nip23 variant present");
        let todo = out.find("FixtureTodoCore(").expect("app variant present");
        assert!(
            nip01 < nip23 && nip23 < todo,
            "variants must appear protocol-first, app-last:\n{out}"
        );
        // Each module variant carries the `ViewSpec` module type for this enum.
        assert!(out.contains("NmpNip01(nmp_nip01::ViewSpec),"));
        assert!(out.contains("NmpNip22(nmp_nip22::ViewSpec),"));
    }

    #[test]
    fn enum_file_namespace_arms_use_the_unmodified_module_name() {
        // The `namespace()` string for each arm is the raw manifest module
        // name (dash form), NOT the UpperCamelCase variant identifier — those
        // are deliberately different and a refactor must not conflate them.
        let out = enum_file(
            "AppUpdate",
            "nmp_core::KernelUpdate",
            &manifest(&["nmp-nip22"], &[]),
            "Update",
            "pub fn namespace(&self) -> &'static str",
        );
        assert!(out.contains("Self::NmpNip22(_) => \"nmp-nip22\","));
        assert!(
            !out.contains("=> \"NmpNip22\""),
            "namespace string must be the dash-form crate name, not the variant ident"
        );
    }

    #[test]
    fn enum_file_is_deterministic_for_identical_input() {
        // Same manifest in → byte-identical source out. No map iteration, no
        // clock, no env — the generator's core invariant.
        let m = manifest(&["nmp-nip01", "nmp-nip22"], &["fixture-todo-core"]);
        let a = enum_file("AppAction", "nmp_core::KernelAction", &m, "Action", "pub fn namespace(&self) -> &'static str");
        let b = enum_file("AppAction", "nmp_core::KernelAction", &m, "Action", "pub fn namespace(&self) -> &'static str");
        assert_eq!(a, b);
    }

    #[test]
    fn const_list_with_zero_modules_emits_an_empty_slice() {
        // Zero modules → `&[]` with no values. The const is still declared so
        // the generated crate compiles.
        let out = const_list("DOMAIN_MODULE_CRATES", &manifest(&[], &[]));
        assert_eq!(out, "pub const DOMAIN_MODULE_CRATES: &[&str] = &[];\n");
    }

    #[test]
    fn const_list_emits_quoted_module_names_in_ordered_sequence() {
        // Non-empty: every module name, quoted, comma-separated, in
        // `ordered_modules()` order (protocol then app).
        let out = const_list(
            "CAPABILITY_MODULE_CRATES",
            &manifest(&["nmp-nip01"], &["fixture-todo-core"]),
        );
        assert_eq!(
            out,
            "pub const CAPABILITY_MODULE_CRATES: &[&str] = &[\"nmp-nip01\", \"fixture-todo-core\"];\n"
        );
    }

    #[test]
    fn cargo_toml_lists_every_module_as_a_path_dependency() {
        // The generated Cargo.toml must declare one `[dependencies]` entry per
        // module, each using the `package = "<dash-name>"` + relative path
        // form, keyed by the underscored crate identifier.
        let out = cargo_toml(&manifest(&["nmp-nip22"], &["fixture-todo-core"]));
        assert!(out.contains("name = \"nmp-app-fixture\""));
        assert!(out.contains(
            "nmp_nip22 = { package = \"nmp-nip22\", path = \"../../../crates/nmp-nip22\" }"
        ));
        assert!(out.contains(
            "fixture_todo_core = { package = \"fixture-todo-core\", path = \"../../../crates/fixture-todo-core\" }"
        ));
    }

    #[test]
    fn cargo_toml_nmp_core_dep_has_no_extra_features() {
        // After PR #356, `nmp_app_new` / `nmp_app_free` / `nmp_app_dispatch_action`
        // are under the default `native` feature — no explicit feature flag needed.
        let out = cargo_toml(&manifest(&[], &["fixture-todo-core"]));
        assert!(
            out.contains("nmp-core = { path = \"../../../crates/nmp-core\" }"),
            "generated nmp-core dep must use plain path dep (no feature flags):\n{out}"
        );
    }

    #[test]
    fn lib_rs_is_constant_and_wires_every_generated_module() {
        // `lib_rs()` takes no manifest — it is a fixed string. It must declare
        // and re-export each generated module so the app crate's public API is
        // stable.
        let out = lib_rs();
        for module in [
            "action", "capability", "domain", "envelope", "ffi", "update", "view_spec",
        ] {
            assert!(out.contains(&format!("pub mod {module};")), "missing mod {module}");
        }
        assert!(out.contains("pub use action::AppAction;"));
        assert!(out.contains("pub use envelope::UpdateEnvelope;"));
        // Deterministic: it is a literal, so two calls are identical.
        assert_eq!(lib_rs(), lib_rs());
    }

    #[test]
    fn envelope_rs_pins_the_tagged_union_wire_contract() {
        // The host update-channel envelope must use the canonical t/v
        // snake_case tagging and carry the snapshot + panic arms. This mirrors
        // `tests/determinism.rs` but at the pure-formatter level, so a refactor
        // of `envelope_rs` is caught without disk I/O.
        let out = envelope_rs();
        assert!(out.contains(r#"#[serde(tag = "t", content = "v", rename_all = "snake_case")]"#));
        assert!(out.contains("Snapshot(serde_json::Value),"));
        assert!(out.contains("Panic(nmp_core::PanicFrame),"));
        // The discrete-update arm (`Update(nmp_core::DeltaEnvelope)`) was
        // deleted as shipped-but-inert — every host bridge only consumed
        // snapshots, and the kernel no longer emits a discrete frame.
        assert!(
            !out.contains("DeltaEnvelope"),
            "envelope must NOT carry the deleted Update arm: {out}"
        );
    }
}
