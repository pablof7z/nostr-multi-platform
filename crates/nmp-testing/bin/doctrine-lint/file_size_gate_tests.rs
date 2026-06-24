//! File-size baseline ratchet smoke tests. These drive the real shell gate
//! through isolated fixtures so doctrine_lint_smoke covers baseline integrity.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::workspace_root;

fn case_dir(name: &str) -> PathBuf {
    let root = workspace_root();
    let dir = root.join("target/file-size-gate-ratchet").join(name);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("fixture dir must be creatable");
    dir
}

fn write_lines(path: &Path, lines: usize) {
    let mut body = String::new();
    for _ in 0..lines {
        body.push_str("// fixture\n");
    }
    fs::write(path, body).expect("fixture file must be writable");
}

fn run_gate(args: &[String]) -> (i32, String) {
    let root = workspace_root();
    let output = Command::new("bash")
        .current_dir(&root)
        .arg(".githooks/check-file-size.sh")
        .args(args)
        .output()
        .expect("file-size gate must spawn");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    (output.status.code().unwrap_or(-1), combined)
}

fn scoped_args(baseline: &str, rel: &str, force_include: bool) -> Vec<String> {
    let mut args = vec![
        "--from-ref".into(),
        "HEAD".into(),
        "--to-ref".into(),
        "HEAD".into(),
        "--baseline-file".into(),
        baseline.into(),
    ];
    if force_include {
        args.push("--force-include".into());
        args.push(rel.into());
    }
    args
}

#[test]
fn baseline_entry_below_hard_cap_fails() {
    let dir = case_dir("below-hard-cap");
    let rel = "target/file-size-gate-ratchet/below-hard-cap/shrunk.kt";
    write_lines(&workspace_root().join(rel), 499);
    let baseline = dir.join("baseline");
    fs::write(&baseline, format!("{rel}\t600\n")).expect("baseline must be writable");

    let (code, output) = run_gate(&scoped_args(baseline.to_str().unwrap(), rel, true));

    assert_ne!(code, 0, "{output}");
    assert!(
        output.contains("STALE baseline entry below hard cap"),
        "{output}"
    );
}

#[test]
fn over_limit_baseline_above_current_without_reason_fails() {
    let dir = case_dir("above-current-without-reason");
    let rel = "target/file-size-gate-ratchet/above-current-without-reason/stale.kt";
    write_lines(&workspace_root().join(rel), 501);
    let baseline = dir.join("baseline");
    fs::write(&baseline, format!("{rel}\t550\n")).expect("baseline must be writable");

    let (code, output) = run_gate(&scoped_args(baseline.to_str().unwrap(), rel, true));

    assert_ne!(code, 0, "{output}");
    assert!(
        output.contains("STALE baseline entry above current LOC"),
        "{output}"
    );
}

#[test]
fn staged_reason_allows_over_limit_baseline_above_current() {
    let dir = case_dir("staged-reason");
    let rel = "target/file-size-gate-ratchet/staged-reason/staged.kt";
    write_lines(&workspace_root().join(rel), 501);
    let baseline = dir.join("baseline");
    fs::write(&baseline, format!("{rel}\t550\tstaged:#1942\n")).expect("baseline must be writable");

    let (code, output) = run_gate(&scoped_args(baseline.to_str().unwrap(), rel, true));

    assert_eq!(code, 0, "{output}");
    assert!(output.contains("STAGED baseline ratchet debt"), "{output}");
}

#[test]
fn ignored_baseline_entries_remain_exempt() {
    let dir = case_dir("ignored-entry");
    let rel = "target/file-size-gate-ratchet/ignored-entry/generated.kt";
    write_lines(&workspace_root().join(rel), 100);
    let baseline = dir.join("baseline");
    fs::write(&baseline, format!("{rel}\t600\n")).expect("baseline must be writable");

    let (code, output) = run_gate(&scoped_args(baseline.to_str().unwrap(), rel, false));

    assert_eq!(code, 0, "{output}");
    assert!(!output.contains("STALE baseline entry"), "{output}");
}
