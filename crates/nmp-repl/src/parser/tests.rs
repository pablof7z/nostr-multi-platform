use std::time::Duration;

use super::*;
use crate::ast::{BudgetPatch, Command, FilterAst, RefreshScope, SeedInput, ShowTopic, Value, VarName};

#[test]
fn empty_line_is_noop() {
    assert_eq!(parse_line("").unwrap(), Command::Noop);
    assert_eq!(parse_line("   ").unwrap(), Command::Noop);
}

#[test]
fn unknown_verb_is_error() {
    let err = parse_line("frobnicate").unwrap_err();
    assert!(err.contains("unknown verb 'frobnicate'"), "got {err}");
}

#[test]
fn set_seed_forms() {
    assert_eq!(
        parse_line("set-seed _@f7z.io").unwrap(),
        Command::SetSeed(SeedInput::Nip05("_@f7z.io".to_string()))
    );
    assert_eq!(
        parse_line("set-seed npub1l2vyh47mk2p0qlsku7hg0vn29faehy9hy34ygaclpn66ukqp3afqutajft")
            .unwrap(),
        Command::SetSeed(SeedInput::Npub(
            "npub1l2vyh47mk2p0qlsku7hg0vn29faehy9hy34ygaclpn66ukqp3afqutajft".to_string()
        ))
    );
    let hex = "fa984bd7dbb282f07e16e7ae87b26a2a7b9b9077b8a5d6c10d3c84d54f76d2a1";
    assert_eq!(
        parse_line(&format!("set-seed {hex}")).unwrap(),
        Command::SetSeed(SeedInput::Hex(hex.to_string()))
    );
}

#[test]
fn set_seed_invalid() {
    let err = parse_line("set-seed not-a-thing").unwrap_err();
    assert!(err.starts_with("parse error:"));
    let err = parse_line("set-seed").unwrap_err();
    assert!(err.contains("exactly one argument"));
}

#[test]
fn req_kinds_and_authors() {
    let cmd = parse_line("req kinds=1,6 authors=$follows").unwrap();
    let Command::Req(f) = cmd else { panic!("not Req") };
    assert_eq!(f.kinds, Some(vec![1, 6]));
    assert_eq!(
        f.authors,
        Some(vec![Value::Var("follows".to_string())])
    );
}

#[test]
fn req_unknown_field() {
    let err = parse_line("req kinds=1 foo=bar").unwrap_err();
    assert!(err.contains("unknown field 'foo'"), "got {err}");
}

#[test]
fn req_bad_kind() {
    let err = parse_line("req kinds=abc").unwrap_err();
    assert!(err.contains("expected integer"), "got {err}");
}

#[test]
fn req_bad_since() {
    let err = parse_line("req kinds=1 since=tomorrow").unwrap_err();
    assert!(err.contains("YYYY-MM-DD"), "got {err}");
}

#[test]
fn req_since_date_and_ts() {
    let cmd = parse_line("req kinds=1 since=2026-01-01").unwrap();
    let Command::Req(f) = cmd else { panic!() };
    assert!(f.since.is_some());
    let cmd = parse_line("req kinds=1 since=1700000000").unwrap();
    let Command::Req(f) = cmd else { panic!() };
    assert_eq!(f.since, Some(1700000000));
}

#[test]
fn req_bad_tag_letter() {
    let err = parse_line("req kinds=1 #tags=x").unwrap_err();
    assert!(err.contains("single letter"), "got {err}");
}

#[test]
fn req_empty_value_list() {
    let err = parse_line("req kinds=").unwrap_err();
    assert!(err.contains("at least one value required"), "got {err}");
}

#[test]
fn req_missing_required() {
    let err = parse_line("req since=2026-01-01").unwrap_err();
    assert!(
        err.contains("at least one of 'kinds'"),
        "got {err}"
    );
}

#[test]
fn req_tag_field() {
    let cmd = parse_line("req kinds=1 #t=bitcoin,nostr").unwrap();
    let Command::Req(f) = cmd else { panic!() };
    let t = f.tags.get(&'t').unwrap();
    assert_eq!(
        t,
        &vec![
            Value::Lit("bitcoin".to_string()),
            Value::Lit("nostr".to_string())
        ]
    );
}

#[test]
fn req_bare_token_rejected() {
    let err = parse_line("req kinds=1 random_token").unwrap_err();
    assert!(err.contains("no bare tokens"), "got {err}");
}

#[test]
fn show_variants() {
    assert_eq!(
        parse_line("show").unwrap(),
        Command::Show(ShowTopic::State)
    );
    assert_eq!(
        parse_line("show relays").unwrap(),
        Command::Show(ShowTopic::Relays)
    );
    assert!(parse_line("show garbage").is_err());
}

#[test]
fn set_app_relays() {
    let cmd = parse_line("set-app-relays wss://a.example,wss://b.example").unwrap();
    assert_eq!(
        cmd,
        Command::SetAppRelays(vec![
            "wss://a.example".to_string(),
            "wss://b.example".to_string()
        ])
    );
    assert!(parse_line("set-app-relays http://bad").is_err());
    assert!(parse_line("set-app-relays").is_err());
}

#[test]
fn set_budget() {
    let cmd = parse_line("set-budget max_connections=50 max_per_user=3 wall=30s").unwrap();
    let Command::SetBudget(p) = cmd else { panic!() };
    assert_eq!(p.max_connections, Some(50));
    assert_eq!(p.max_per_user, Some(3));
    assert_eq!(p.wall, Some(Duration::from_secs(30)));
}

#[test]
fn set_budget_ms_and_m() {
    let Command::SetBudget(p) = parse_line("set-budget wall=500ms").unwrap() else {
        panic!()
    };
    assert_eq!(p.wall, Some(Duration::from_millis(500)));
    let Command::SetBudget(p) = parse_line("set-budget wall=2m").unwrap() else {
        panic!()
    };
    assert_eq!(p.wall, Some(Duration::from_secs(120)));
}

#[test]
fn refresh_scopes() {
    assert_eq!(
        parse_line("refresh").unwrap(),
        Command::Refresh(RefreshScope::All)
    );
    assert_eq!(
        parse_line("refresh follows").unwrap(),
        Command::Refresh(RefreshScope::Follows)
    );
    assert_eq!(
        parse_line("refresh mailboxes").unwrap(),
        Command::Refresh(RefreshScope::Mailboxes)
    );
    assert!(parse_line("refresh garbage").is_err());
}

#[test]
fn expand_var() {
    assert_eq!(
        parse_line("expand $follows").unwrap(),
        Command::Expand(VarName("follows".to_string()))
    );
    assert!(parse_line("expand follows").is_err());
    assert!(parse_line("expand $").is_err());
}

#[test]
fn quit_aliases() {
    assert_eq!(parse_line("quit").unwrap(), Command::Quit);
    assert_eq!(parse_line("exit").unwrap(), Command::Quit);
}

#[test]
fn help_topics() {
    assert_eq!(parse_line("help").unwrap(), Command::Help(None));
    assert_eq!(
        parse_line("help req").unwrap(),
        Command::Help(Some("req".to_string()))
    );
}

#[test]
fn req_authors_mixed_literals_and_vars() {
    let hex = "fa984bd7dbb282f07e16e7ae87b26a2a7b9b9077b8a5d6c10d3c84d54f76d2a1";
    let cmd = parse_line(&format!("req kinds=1 authors=$me,{hex}")).unwrap();
    let Command::Req(f) = cmd else { panic!() };
    assert_eq!(
        f.authors,
        Some(vec![
            Value::Var("me".to_string()),
            Value::Lit(hex.to_string())
        ])
    );
}

#[test]
fn control_chars_rejected() {
    let err = parse_line("req \x07 kinds=1").unwrap_err();
    assert!(err.contains("control character"), "got {err}");
}

// ── MLS / Marmot verbs ──────────────────────────────────────────────

#[test]
fn create_account_optional_name() {
    assert_eq!(
        parse_line("create-account").unwrap(),
        Command::CreateAccount(None, vec![])
    );
    assert_eq!(
        parse_line("create-account alice").unwrap(),
        Command::CreateAccount(Some("alice".to_string()), vec![])
    );
    assert_eq!(
        parse_line("create-account alice wss://relay.primal.net").unwrap(),
        Command::CreateAccount(
            Some("alice".to_string()),
            vec!["wss://relay.primal.net".to_string()]
        )
    );
    assert_eq!(
        parse_line("create-account wss://relay.primal.net").unwrap(),
        Command::CreateAccount(None, vec!["wss://relay.primal.net".to_string()])
    );
}

#[test]
fn load_key_requires_arg() {
    assert_eq!(
        parse_line("load-key nsec1abc").unwrap(),
        Command::LoadKey("nsec1abc".to_string())
    );
    assert!(parse_line("load-key").is_err());
}

#[cfg(feature = "mls")]
#[test]
fn mls_nullary_verbs() {
    assert_eq!(parse_line("mls-init").unwrap(), Command::MlsInit);
    assert_eq!(parse_line("mls-status").unwrap(), Command::MlsStatus);
    assert!(parse_line("mls-init foo").is_err());
}

#[cfg(feature = "mls")]
#[test]
fn mls_create_requires_name() {
    assert_eq!(
        parse_line("mls-create TestGroup").unwrap(),
        Command::MlsCreate("TestGroup".to_string())
    );
    assert!(parse_line("mls-create").is_err());
}

#[cfg(feature = "mls")]
#[test]
fn mls_invite_two_args() {
    assert_eq!(
        parse_line("mls-invite abcd npub1xyz").unwrap(),
        Command::MlsInvite("abcd".to_string(), "npub1xyz".to_string())
    );
    assert!(parse_line("mls-invite abcd").is_err());
    assert!(parse_line("mls-invite").is_err());
}

#[cfg(feature = "mls")]
#[test]
fn mls_accept_optional() {
    assert_eq!(parse_line("mls-accept").unwrap(), Command::MlsAccept(None));
    assert_eq!(
        parse_line("mls-accept deadbeef").unwrap(),
        Command::MlsAccept(Some("deadbeef".to_string()))
    );
}

#[cfg(feature = "mls")]
#[test]
fn mls_send_joins_message_text() {
    assert_eq!(
        parse_line("mls-send abcd hello bob").unwrap(),
        Command::MlsSend("abcd".to_string(), "hello bob".to_string())
    );
    assert_eq!(
        parse_line("mls-send abcd one").unwrap(),
        Command::MlsSend("abcd".to_string(), "one".to_string())
    );
    assert!(parse_line("mls-send abcd").is_err());
    assert!(parse_line("mls-send").is_err());
}

#[cfg(feature = "mls")]
#[test]
fn mls_messages_requires_group() {
    assert_eq!(
        parse_line("mls-messages deadbeef").unwrap(),
        Command::MlsMessages("deadbeef".to_string())
    );
    assert!(parse_line("mls-messages").is_err());
}

#[cfg(feature = "mls")]
#[test]
fn mls_fetch_kp_requires_npub() {
    assert_eq!(
        parse_line("mls-fetch-kp npub1xyz").unwrap(),
        Command::MlsFetchKp("npub1xyz".to_string())
    );
    assert!(parse_line("mls-fetch-kp").is_err());
}

/// Without `--features mls`, the `mls-*` verbs should fall through to the
/// unknown-verb error from `parse_line`. (When the feature IS on, this
/// test is skipped — the dedicated tests above cover that path.)
#[cfg(not(feature = "mls"))]
#[test]
fn mls_verbs_unknown_without_feature() {
    for verb in [
        "mls-init",
        "mls-status",
        "mls-create",
        "mls-fetch-kp",
        "mls-invite",
        "mls-accept",
        "mls-send",
        "mls-messages",
    ] {
        let err = parse_line(verb).unwrap_err();
        assert!(
            err.contains("unknown verb"),
            "expected 'unknown verb' for '{verb}', got: {err}"
        );
    }
}
