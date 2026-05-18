use crate::{app_crate_name, rust_crate_name, variant_name, AppManifest};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GenerationReport {
    pub app_name: String,
    pub crate_name: String,
    pub files: Vec<PathBuf>,
}

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
/// `{"t":"update","v":<KernelUpdate>}` or `{"t":"snapshot","v":<snapshot>}` —
/// so the host decodes exactly **one** discriminated type. This MUST stay
/// byte-identical to `nmp_core::UpdateEnvelope`'s serde contract (tag `t`,
/// content `v`, snake_case variants); see
/// `docs/design/0001-ffi-update-channel-envelope.md`.
///
/// The discrete arm wraps `nmp_core::KernelUpdate` **directly** (not the
/// projected `AppUpdate`): only `Kernel(_)` discrete updates ever flow on the
/// streaming channel — module-projected `AppUpdate` variants return through
/// `FfiApp::dispatch`, not `update_tx`. Carrying module updates here later is
/// purely **additive** (a new snake_case variant on the same `t`
/// discriminator). The snapshot interior is intentionally opaque
/// (`serde_json::Value`) — this type models the discriminator, not the
/// snapshot's ~30 internal fields.
fn envelope_rs() -> String {
    [
        "use serde::{Deserialize, Serialize};",
        "",
        "#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]",
        "#[serde(tag = \"t\", content = \"v\", rename_all = \"snake_case\")]",
        "pub enum UpdateEnvelope {",
        "    /// A discrete update — apply as a delta.",
        "    Update(nmp_core::KernelUpdate),",
        "    /// A full snapshot — replace rendered state.",
        "    Snapshot(serde_json::Value),",
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
    format!(
        "use crate::{{AppAction, AppUpdate}};\n\n#[derive(Default)]\npub struct FfiApp {{\n    rev: u64,\n}}\n\nimpl FfiApp {{\n    pub fn new() -> Self {{\n        Self::default()\n    }}\n\n    pub fn app_name(&self) -> &'static str {{\n        \"{}\"\n    }}\n\n    pub fn dispatch(&mut self, action: AppAction) -> AppUpdate {{\n        self.rev = self.rev.saturating_add(1);\n        AppUpdate::Kernel(nmp_core::KernelUpdate::Diagnostics {{\n            summary: format!(\"dispatched {{}} at rev {{}}\", action.namespace(), self.rev),\n        }})\n    }}\n}}\n",
        manifest.name
    )
}
