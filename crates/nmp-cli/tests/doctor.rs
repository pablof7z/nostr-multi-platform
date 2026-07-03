mod helpers;

use std::fs;
use std::path::Path;

use helpers::{nmp, TempDir};

const REMOTE: &str = "https://github.com/pablof7z/nostr-multi-platform";

#[test]
fn d01_passes_when_all_nmp_deps_use_path_source() {
    let root = valid_path_app("d01-pass");
    let output = run_doctor(root.path(), &[]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(!stdout(&output).contains("D01"));
}

#[test]
fn d01_fails_on_mixed_nmp_sources() {
    let root = TempDir::new("doctor-d01-fail");
    write_base_nmp(root.path(), r#"protocol = ["nmp-nip50"]"#);
    write_manifest(
        root.path(),
        r#"
        [package]
        name = "fixture-app"
        version = "0.1.0"
        edition = "2021"

        [dependencies]
        nmp-core = { path = "vendor/nmp-core" }
        nmp-nip50 = { git = "https://github.com/pablof7z/nostr-multi-platform", tag = "v1.0.0" }
        "#,
    );
    write_crate(root.path(), "nmp-core");
    write_lock(
        root.path(),
        &[path_lock("nmp-core"), git_lock("nmp-nip50", "v1.0.0")],
    );
    let output = run_doctor(root.path(), &[]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stdout(&output).contains("D01"));
}

#[test]
fn d02_passes_when_lockfile_matches_manifest_sources() {
    let root = valid_path_app("d02-pass");
    let output = run_doctor(root.path(), &[]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(!stdout(&output).contains("D02"));
}

#[test]
fn d02_fails_when_lockfile_is_stale() {
    let root = valid_path_app("d02-fail");
    write_lock(
        root.path(),
        &[git_lock("nmp-core", "v1.0.0"), path_lock("nmp-nip50")],
    );
    let output = run_doctor(root.path(), &[]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stdout(&output).contains("D02"));
}

#[test]
fn d03_reads_retired_crates_from_release_manifest() {
    let root = valid_path_app("d03-fail");
    append_dep(root.path(), r#"nmp-ffi = { path = "vendor/nmp-ffi" }"#);
    write_crate(root.path(), "nmp-ffi");
    write_lock(
        root.path(),
        &[
            path_lock("nmp-core"),
            path_lock("nmp-nip50"),
            path_lock("nmp-ffi"),
        ],
    );
    let output = run_doctor(root.path(), &[]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stdout(&output).contains("D03"));
    assert!(stdout(&output).contains("nmp-ffi"));
}

#[test]
fn d03_passes_for_active_crates() {
    let root = valid_path_app("d03-pass");
    let output = run_doctor(root.path(), &[]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(!stdout(&output).contains("D03"));
}

#[test]
fn d04_passes_for_matching_path_crates() {
    let root = valid_path_app("d04-pass");
    let output = run_doctor(root.path(), &[]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(!stdout(&output).contains("D04"));
}

#[test]
fn d04_fails_when_path_package_name_differs() {
    let root = valid_path_app("d04-fail");
    write_crate_named(root.path(), "nmp-nip50", "wrong-name");
    let output = run_doctor(root.path(), &[]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stdout(&output).contains("D04"));
}

#[test]
fn d05_always_reports_current_baseline() {
    let root = valid_path_app("d05-report");
    let output = run_doctor(root.path(), &["--json"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stdout(&output).contains("\"id\":\"D05\""));
    assert!(stdout(&output).contains("\"level\":\"info\""));
}

#[test]
fn d06_passes_when_companions_share_a_pin() {
    let root = companion_app("d06-pass", "v1.0.0", "v1.0.0");
    let output = run_doctor(root.path(), &[]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(!stdout(&output).contains("D06"));
}

#[test]
fn d06_warns_on_companion_pin_drift_and_strict_fails() {
    let root = companion_app("d06-fail", "v1.0.0", "v2.0.0");
    let output = run_doctor(root.path(), &[]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stdout(&output).contains("warning:"));
    assert!(stdout(&output).contains("D06"));

    let strict = run_doctor(root.path(), &["--strict"]);
    assert_eq!(strict.status.code(), Some(1));
    assert!(stdout(&strict).contains("error:"));
}

#[test]
fn d07_passes_when_modules_are_declared_as_dependencies() {
    let root = valid_path_app("d07-pass");
    let output = run_doctor(root.path(), &[]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(!stdout(&output).contains("D07"));
}

#[test]
fn d07_fails_when_nmp_toml_references_missing_crate() {
    let root = valid_path_app("d07-fail");
    write_base_nmp(root.path(), r#"protocol = ["nmp-missing"]"#);
    let output = run_doctor(root.path(), &[]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stdout(&output).contains("D07"));
}

#[test]
fn unreadable_or_unparseable_manifest_exits_two() {
    let root = TempDir::new("doctor-parse-fail");
    fs::write(root.path().join("nmp.toml"), "[app\n").unwrap();
    let output = run_doctor(root.path(), &[]);
    assert_eq!(output.status.code(), Some(2));
}

fn valid_path_app(tag: &str) -> TempDir {
    let root = TempDir::new(&format!("doctor-{tag}"));
    write_base_nmp(root.path(), r#"protocol = ["nmp-nip50"]"#);
    write_manifest(
        root.path(),
        r#"
        [package]
        name = "fixture-app"
        version = "0.1.0"
        edition = "2021"

        [dependencies]
        nmp-core = { path = "vendor/nmp-core" }
        nmp-nip50 = { path = "vendor/nmp-nip50" }
        "#,
    );
    write_crate(root.path(), "nmp-core");
    write_crate(root.path(), "nmp-nip50");
    write_lock(
        root.path(),
        &[path_lock("nmp-core"), path_lock("nmp-nip50")],
    );
    root
}

fn companion_app(tag: &str, nip46_tag: &str, runtime_tag: &str) -> TempDir {
    let root = TempDir::new(&format!("doctor-{tag}"));
    fs::write(
        root.path().join("nmp.toml"),
        r#"
        [app]
        name = "fixture"

        [modules]
        kernel = "nmp-core"
        protocol = ["nmp-nip46", "nmp-nip46-runtime"]
        app = []

        [companions]
        signing = ["nmp-nip46", "nmp-nip46-runtime"]
        "#,
    )
    .unwrap();
    write_manifest(
        root.path(),
        &format!(
            r#"
            [package]
            name = "fixture-app"
            version = "0.1.0"
            edition = "2021"

            [dependencies]
            nmp-core = {{ git = "{REMOTE}", tag = "v1.0.0" }}
            nmp-nip46 = {{ git = "{REMOTE}", tag = "{nip46_tag}" }}
            nmp-nip46-runtime = {{ git = "{REMOTE}", tag = "{runtime_tag}" }}
            "#
        ),
    );
    write_lock(
        root.path(),
        &[
            git_lock("nmp-core", "v1.0.0"),
            git_lock("nmp-nip46", nip46_tag),
            git_lock("nmp-nip46-runtime", runtime_tag),
        ],
    );
    root
}

fn write_base_nmp(root: &Path, protocol_line: &str) {
    fs::write(
        root.join("nmp.toml"),
        format!(
            r#"
            [app]
            name = "fixture"

            [modules]
            kernel = "nmp-core"
            {protocol_line}
            app = []
            "#
        ),
    )
    .unwrap();
}

fn write_manifest(root: &Path, body: &str) {
    fs::write(root.join("Cargo.toml"), unindent(body)).unwrap();
}

fn append_dep(root: &Path, line: &str) {
    let path = root.join("Cargo.toml");
    let mut body = fs::read_to_string(&path).unwrap();
    body.push_str(line);
    body.push('\n');
    fs::write(path, body).unwrap();
}

fn write_crate(root: &Path, name: &str) {
    write_crate_named(root, name, name);
}

fn write_crate_named(root: &Path, dir: &str, package: &str) {
    let dir = root.join("vendor").join(dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("Cargo.toml"),
        format!(
            r#"
            [package]
            name = "{package}"
            version = "0.1.0"
            edition = "2021"
            "#
        ),
    )
    .unwrap();
}

fn write_lock(root: &Path, packages: &[String]) {
    fs::write(
        root.join("Cargo.lock"),
        format!("version = 3\n\n{}", packages.join("\n")),
    )
    .unwrap();
}

fn path_lock(name: &str) -> String {
    format!(
        r#"[[package]]
name = "{name}"
version = "0.1.0"
"#
    )
}

fn git_lock(name: &str, tag: &str) -> String {
    format!(
        r#"[[package]]
name = "{name}"
version = "0.1.0"
source = "git+{REMOTE}?tag={tag}#0000000000000000000000000000000000000000"
"#
    )
}

fn run_doctor(root: &Path, extra: &[&str]) -> std::process::Output {
    let mut args = vec!["doctor"];
    args.extend_from_slice(extra);
    nmp(root, &args)
}

fn stdout(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn unindent(input: &str) -> String {
    input
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}
