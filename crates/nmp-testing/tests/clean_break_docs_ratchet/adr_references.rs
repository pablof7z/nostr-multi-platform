use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repo root is two levels above crates/nmp-testing")
        .to_path_buf()
}

fn rel_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn git_tracked_text_inputs(root: &Path) -> Vec<PathBuf> {
    let output = Command::new("git")
        .arg("ls-files")
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
        .filter(|line| {
            matches!(
                Path::new(line).extension().and_then(|ext| ext.to_str()),
                Some(
                    "md" | "rs"
                        | "toml"
                        | "sh"
                        | "py"
                        | "ts"
                        | "tsx"
                        | "kt"
                        | "swift"
                        | "yml"
                        | "yaml"
                )
            )
        })
        .map(|line| root.join(line))
        .collect()
}

fn existing_adr_numbers(root: &Path) -> BTreeSet<String> {
    let decisions = root.join("docs/decisions");
    std::fs::read_dir(&decisions)
        .unwrap_or_else(|err| panic!("read {}: {err}", decisions.display()))
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter_map(|name| {
            let number = name.get(0..4)?;
            number
                .chars()
                .all(|ch| ch.is_ascii_digit())
                .then(|| number.to_string())
        })
        .collect()
}

fn four_digits_at(line: &str, idx: usize) -> Option<&str> {
    let end = idx.checked_add(4)?;
    let bytes = line.as_bytes();
    if end <= bytes.len() && bytes[idx..end].iter().all(u8::is_ascii_digit) {
        Some(&line[idx..end])
    } else {
        None
    }
}

fn compact_adr_references(line: &str) -> Vec<(String, Vec<String>)> {
    let mut refs = Vec::new();
    let mut start = 0;

    while let Some(offset) = line[start..].find("ADR-") {
        let idx = start + offset;
        let Some(first) = four_digits_at(line, idx + 4) else {
            start = idx + 4;
            continue;
        };

        let mut numbers = vec![first.to_string()];
        let mut cursor = idx + 8;
        while line[cursor..].starts_with('/') {
            let digit_start = cursor + 1;
            let Some(next) = four_digits_at(line, digit_start) else {
                break;
            };
            numbers.push(next.to_string());
            cursor = digit_start + 4;
        }

        if numbers.len() > 1 {
            refs.push((line[idx..cursor].to_string(), numbers));
        }
        start = cursor.max(idx + 4);
    }

    refs
}

#[test]
fn compact_adr_references_do_not_hide_deleted_decisions() {
    let root = repo_root();
    let existing = existing_adr_numbers(&root);
    let files = git_tracked_text_inputs(&root);
    assert!(!files.is_empty(), "ADR reference ratchet must scan files");

    let mut violations = Vec::new();
    for file in files {
        let rel = rel_path(&root, &file);
        let text = std::fs::read_to_string(&file)
            .unwrap_or_else(|err| panic!("read {}: {err}", file.display()));
        for (line_idx, line) in text.lines().enumerate() {
            for (raw, numbers) in compact_adr_references(line) {
                let missing: Vec<_> = numbers
                    .iter()
                    .filter(|number| !existing.contains(*number))
                    .cloned()
                    .collect();
                if !missing.is_empty() {
                    violations.push(format!(
                        "{}:{}: compact ADR reference `{}` cites deleted/missing ADR number(s): {}",
                        rel,
                        line_idx + 1,
                        raw,
                        missing.join(", ")
                    ));
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "compact slash-form ADR references must not hide deleted decisions. \
         Name the current owner directly, or remove historical deleted-ADR \
         numbers from current guidance.\n{}",
        violations.join("\n")
    );
}
