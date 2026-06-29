//! Raw native ABI reintroduction ratchet for #2489.

use std::path::{Path, PathBuf};
use std::process::Command;

type Pattern = (&'static str, &'static str, &'static str);

const RAW_ABI_SCAN_ROOTS: &[&str] = &["crates"];
const RAW_ABI_SKIP_PREFIXES: &[&str] = &[
    "crates/nmp-testing/",
    "crates/nmp-marmot/",
    "crates/nmp-uniffi/generated/",
];
const RAW_ABI_EXPORT_TOKENS: &[Pattern] = &[
    (
        "nmp_app_",
        "deleted_framework_c_export",
        "deleted reusable framework C exports must not be reintroduced; native public API is UniFFI",
    ),
    (
        "nmp_signer_broker_",
        "deleted_framework_c_export",
        "deleted reusable framework C exports must not be reintroduced; native public API is UniFFI",
    ),
    (
        "nmp_external_signer_",
        "deleted_framework_c_export",
        "deleted reusable framework C exports must not be reintroduced; native public API is UniFFI",
    ),
    (
        "nmp_mirror_",
        "deleted_framework_c_export",
        "deleted reusable framework C exports must not be reintroduced; native public API is UniFFI",
    ),
    (
        "nmp_content_",
        "deleted_framework_c_export",
        "deleted reusable framework C exports must not be reintroduced; native public API is UniFFI",
    ),
    (
        "nmp_nip21_",
        "deleted_framework_c_export",
        "deleted reusable framework C exports must not be reintroduced; native public API is UniFFI",
    ),
    (
        "nmp_free_string",
        "deleted_framework_c_export",
        "deleted reusable framework C exports must not be reintroduced; native public API is UniFFI",
    ),
];

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repo root is two levels above crates/nmp-testing")
        .to_path_buf()
}

fn git_tracked_under(root: &Path, roots: &[&str]) -> Vec<PathBuf> {
    let output = Command::new("git")
        .arg("ls-files")
        .arg("--")
        .args(roots)
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
        .map(|line| root.join(line))
        .filter(|path| raw_abi_scan_file_in_scope(root, path))
        .collect()
}

fn raw_abi_scan_file_in_scope(root: &Path, path: &Path) -> bool {
    let rel = rel_path(root, path);
    !RAW_ABI_SKIP_PREFIXES
        .iter()
        .any(|prefix| rel.starts_with(prefix))
}

fn rel_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn raw_framework_export_line(line: &str, token: &str) -> bool {
    let trimmed = line.trim_start();
    if trimmed.starts_with("//") || trimmed.starts_with('*') || trimmed.starts_with("/*") {
        return false;
    }
    line.contains("extern \"C\"") && line.contains("fn ") && line.contains(token)
}

#[test]
fn framework_crates_do_not_reintroduce_deleted_public_raw_native_exports() {
    let root = repo_root();
    let files = git_tracked_under(&root, RAW_ABI_SCAN_ROOTS);
    assert!(
        !files.is_empty(),
        "raw ABI ratchet must scan framework crates"
    );

    let mut violations = Vec::new();
    for file in files {
        let rel = rel_path(&root, &file);
        let bytes =
            std::fs::read(&file).unwrap_or_else(|err| panic!("read {}: {err}", file.display()));
        let text = String::from_utf8_lossy(&bytes);
        for (line_idx, line) in text.lines().enumerate() {
            for &(token, label, guidance) in RAW_ABI_EXPORT_TOKENS {
                if raw_framework_export_line(line, token) {
                    violations.push(format!(
                        "{}:{}: error[raw_native_abi:{}]: `{}` - {}\n    {}",
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
        "framework crates contain deleted public raw native ABI exports. \
         The reusable native public surface is UniFFI; app-owned Gallery glue, \
         Marmot history, generated UniFFI internals, and nmp-testing fixtures \
         are outside this ratchet.\n{}",
        violations.join("\n")
    );
}
