//! Smoke tests for D26 — no ambient authority in protocol/command code
//! (Workstream D item 7; the K2 + D6 capability-honesty lock-in). D26 bans, in
//! protocol/command code, the broad `AppHost` super-trait (narrow modules must
//! take the specific registrar/capability traits) and a protocol command
//! reaching the raw `active_local_keys` signing keys (signing goes through the
//! signer-session port only).
//!
//! Split out of `tests.rs` (file-size hard cap); the shared
//! `run_lint`/`workspace_root`/`fixture_path` helpers live in the parent
//! integration-test module and are imported via `super`.

use super::{fixture_path, run_lint, workspace_root};

/// Stage `fixtures/d26/<which>.rs` in an isolated `target/<label>/` dir so the
/// sibling fixture cannot pollute the assertion, run the lint with the
/// `--d26-extra-scope <label>` opt-in (which activates BOTH the `AppHost` and
/// `active_local_keys` sub-scopes), and return `(exit_code, stdout, stderr)`.
fn run_isolated(which: &str, label: &str) -> (i32, String, String) {
    let workspace = workspace_root();
    let tmp = workspace.join("target").join(label);
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).expect("create temp dir");
    let src = workspace.join(fixture_path(&format!("d26/{}.rs", which)));
    std::fs::copy(&src, tmp.join(format!("{}.rs", which))).expect("copy fixture");
    let tmp_str = tmp.to_string_lossy().into_owned();
    run_lint(&["--path", &tmp_str, "--d26-extra-scope", label])
}

#[test]
fn d26_positive_fixture_fires() {
    let (code, stdout, stderr) = run_isolated("pos", "doctrine_lint_d26_pos");
    assert_eq!(
        code, 1,
        "d26 positive must exit 1; stdout:\n{}\nstderr:\n{}",
        stdout, stderr
    );
    assert!(
        stdout.contains("error[D26]"),
        "d26 positive must emit >=1 D26 finding; stdout:\n{}",
        stdout
    );
    // The fixture plants 5 banned references: 3 `AppHost` (import + impl-Trait
    // bound + generic bound) and 2 `active_local_keys` reaches (receiver call +
    // bareword call). All must surface.
    let app_host = stdout.matches("composition super-trait").count();
    assert!(
        app_host >= 3,
        "d26 must flag all 3 AppHost references; got {}; stdout:\n{}",
        app_host,
        stdout
    );
    let alk = stdout.matches("signer-session port").count();
    assert!(
        alk >= 2,
        "d26 must flag both active_local_keys reaches; got {}; stdout:\n{}",
        alk,
        stdout
    );
    let total = stdout.matches("error[D26]").count();
    assert!(
        total >= 5,
        "d26 must flag all 5 planted ambient-authority references; got {}; stdout:\n{}",
        total,
        stdout
    );
}

#[test]
fn d26_negative_fixture_clean() {
    let (code, stdout, stderr) = run_isolated("neg", "doctrine_lint_d26_neg");
    assert_eq!(
        code, 0,
        "d26 negative must exit 0; stdout:\n{}\nstderr:\n{}",
        stdout, stderr
    );
    assert!(
        !stdout.contains("error[D26]"),
        "d26 negative must produce zero D26 findings; stdout:\n{}",
        stdout
    );
}

/// Integration guard (Workstream D item 7): every reusable protocol crate's
/// `src/` tree — the protocol-command implementation surface — must be D26-clean.
/// A future protocol command that takes the broad `AppHost` or reaches the raw
/// `active_local_keys` signing keys would fail here. This is the production-facing
/// teeth of the lock-in.
#[test]
fn protocol_crates_are_d26_clean() {
    const PROTOCOL_CRATES: &[&str] = &[
        "nmp-nip01",
        "nmp-nip02",
        "nmp-nip11",
        "nmp-nip17",
        "nmp-nip18",
        "nmp-nip29",
        "nmp-nip42",
        "nmp-nip47",
        "nmp-nip51",
        "nmp-nip57",
        "nmp-nip59",
        "nmp-nip60",
        "nmp-nip77",
        "nmp-marmot",
        "nmp-blossom",
        "nmp-nwc",
        "nmp-router",
        "nmp-wot",
        "nmp-content",
        "nmp-feed",
    ];
    for c in PROTOCOL_CRATES {
        let path = format!("crates/{}/src", c);
        let (_code, stdout, stderr) = run_lint(&["--path", &path]);
        let d26: Vec<&str> = stdout
            .lines()
            .filter(|l| l.contains("error[D26]"))
            .collect();
        assert!(
            d26.is_empty(),
            "{} must be D26-clean — take a narrow registrar/capability trait, not \
             `AppHost`, and sign through the signer-session port, not \
             `active_local_keys`. D26 findings:\n{}\nstderr:\n{}",
            c,
            d26.join("\n"),
            stderr
        );
    }
}

/// Integration guard: the `nmp-core` protocol-command modules (the
/// `ProtocolCommand` framework + actor command handlers) must be `AppHost`-clean.
/// Narrow modules name the specific narrow traits; only the `AppHost` definition
/// (`substrate/app_host/`, out of scope) and the composition root may name the
/// super-trait. `active_local_keys` is NOT gated in `nmp-core` — it hosts the
/// legitimate `LocalSignerAccess` port / `ProtocolCommandContext` accessor that
/// plan Workstream-D item 5 removes separately.
#[test]
fn nmp_core_command_modules_are_d26_clean() {
    for path in [
        "crates/nmp-core/src/substrate/protocol.rs",
        "crates/nmp-core/src/actor/commands",
    ] {
        let (_code, stdout, stderr) = run_lint(&["--path", path]);
        let d26: Vec<&str> = stdout
            .lines()
            .filter(|l| l.contains("error[D26]"))
            .collect();
        assert!(
            d26.is_empty(),
            "{} must be D26-clean (no `AppHost` in protocol-command code). \
             D26 findings:\n{}\nstderr:\n{}",
            path,
            d26.join("\n"),
            stderr
        );
    }
}

#[test]
fn app_host_supertrait_stays_methodless() {
    let root = workspace_root();
    let path = root.join("crates/nmp-core/src/substrate/app_host/mod.rs");
    let body = std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()));
    let start = body
        .find("pub trait AppHost:")
        .expect("AppHost trait definition must exist");
    let after_start = &body[start..];
    let open = after_start
        .find('{')
        .expect("AppHost trait must have an opening brace");
    let after_open = &after_start[open + 1..];
    let close = after_open
        .find("\n}")
        .expect("AppHost trait must have a closing brace");
    let trait_body = &after_open[..close];

    assert!(
        !trait_body.contains("fn "),
        "AppHost must stay a methodless composition super-trait. Add product or \
         builder semantics to a narrow registrar trait, owner installer, or \
         app/runtime composition root instead."
    );
}
