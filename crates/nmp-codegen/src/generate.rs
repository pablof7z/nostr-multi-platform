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
        "[package]\nname = \"{}\"\nversion.workspace = true\nedition.workspace = true\nlicense.workspace = true\n\n[dependencies]\nnmp-core = {{ path = \"../../../crates/nmp-core\" }}\n",
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
        "pub mod ffi;",
        "pub mod update;",
        "pub mod view_spec;",
        "",
        "pub use action::AppAction;",
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
