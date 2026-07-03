use std::fs;
use std::path::Path;

#[test]
fn native_runtime_keeps_search_and_group_deps_optional() {
    let root = super::workspace_root();
    let cargo = fs::read_to_string(root.join("crates/nmp-native-runtime/Cargo.toml")).unwrap();
    assert_optional_dep(&cargo, "nmp-nip50", "search");
    assert_no_production_dep(&cargo, "nmp-nip29");
}

#[test]
fn browser_runtime_keeps_search_and_group_deps_optional() {
    let root = super::workspace_root();
    let cargo = fs::read_to_string(root.join("crates/nmp-browser-runtime/Cargo.toml")).unwrap();
    assert_optional_dep(&cargo, "nmp-nip50", "search");
    assert_optional_dep(&cargo, "nmp-nip29", "groups");
}

#[test]
fn runtime_crates_do_not_define_concept_open_doorways() {
    let root = super::workspace_root();
    let mut findings = Vec::new();
    for rel in [
        "crates/nmp-native-runtime/src",
        "crates/nmp-browser-runtime/src",
    ] {
        collect_forbidden_open_fns(&root.join(rel), &mut findings);
    }
    assert!(
        findings.is_empty(),
        "concept read doorways must live in concept crates:\n{}",
        findings.join("\n")
    );
}

fn assert_optional_dep(cargo: &str, dep: &str, feature: &str) {
    let line = cargo
        .lines()
        .find(|line| line.trim_start().starts_with(&format!("{dep} = ")))
        .unwrap_or_else(|| panic!("{dep} dependency missing"));
    assert!(
        line.contains("optional = true"),
        "{dep} must stay optional in runtime Cargo.toml"
    );
    let feature_line =
        feature_block(cargo, feature).unwrap_or_else(|| panic!("{feature} feature missing"));
    assert!(
        feature_line.contains(&format!("dep:{dep}")),
        "{feature} feature must own the dep:{dep} edge"
    );
}

fn feature_block(cargo: &str, feature: &str) -> Option<String> {
    let mut lines = cargo.lines();
    while let Some(line) = lines.next() {
        if !line.trim_start().starts_with(&format!("{feature} = ")) {
            continue;
        }
        let mut block = line.to_string();
        while !block.contains(']') {
            let Some(next) = lines.next() else {
                break;
            };
            block.push('\n');
            block.push_str(next);
        }
        return Some(block);
    }
    None
}

fn assert_no_production_dep(cargo: &str, dep: &str) {
    let in_dev_dependency = cargo
        .lines()
        .scan(false, |is_dev, line| {
            let trimmed = line.trim();
            if trimmed.starts_with('[') {
                *is_dev = trimmed == "[dev-dependencies]";
            }
            Some((*is_dev, trimmed.to_string()))
        })
        .any(|(is_dev, line)| !is_dev && line.starts_with(&format!("{dep} = ")));
    assert!(
        !in_dev_dependency,
        "{dep} must not be bundled as a production native-runtime dependency"
    );
}

fn collect_forbidden_open_fns(dir: &Path, findings: &mut Vec<String>) {
    if !dir.exists() {
        return;
    }
    for entry in fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_dir() {
            collect_forbidden_open_fns(&path, findings);
            continue;
        }
        if path.extension().and_then(|ext| ext.to_str()) != Some("rs")
            || path.to_string_lossy().contains("/tests/")
        {
            continue;
        }
        let raw = fs::read_to_string(&path).unwrap();
        for (index, line) in raw.lines().enumerate() {
            let line = line.split("//").next().unwrap_or("").trim();
            if forbidden_open_definition(line) {
                findings.push(format!("{}:{}: {}", path.display(), index + 1, line));
            }
        }
    }
}

fn forbidden_open_definition(line: &str) -> bool {
    let Some(after_fn) = line
        .strip_prefix("pub fn ")
        .or_else(|| line.strip_prefix("fn "))
        .or_else(|| line.strip_prefix("pub(crate) fn "))
    else {
        return false;
    };
    [
        "open_search",
        "close_search",
        "open_group",
        "close_group",
        "open_groups",
        "close_groups",
        "open_reactions",
        "close_reactions",
    ]
    .iter()
    .any(|name| after_fn.starts_with(name))
}
