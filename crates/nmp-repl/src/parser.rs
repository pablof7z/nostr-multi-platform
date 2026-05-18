//! Command-line tokenizer + filter parser. Pure function — no I/O, no
//! session reads. See `docs/design/nmp-repl.md` §5.
//!
//! Tokens are whitespace-separated. Tokens of the form `key=val` are filter
//! fields (only valid inside `req`); bare tokens are positional. Variables
//! are tokens beginning with `$`. Comma is the in-value list separator.

use std::time::Duration;

use crate::ast::{
    BudgetPatch, Command, FilterAst, RefreshScope, SeedInput, ShowTopic, Value, VarName,
};

/// Parse a single line of input into a `Command`.
///
/// Empty / whitespace-only input → `Command::Noop`. Returns `Err(String)`
/// with a user-facing error message on parse failure.
pub fn parse_line(line: &str) -> Result<Command, String> {
    // Reject control chars (newline, tab, NUL etc.) anywhere in the input.
    for c in line.chars() {
        if c == '\t' {
            // Treat tab as whitespace — split handles it.
            continue;
        }
        if c.is_control() && c != '\n' && c != '\r' {
            return Err(format!(
                "parse error: control character U+{:04X} not allowed in input",
                c as u32
            ));
        }
    }

    let toks: Vec<&str> = line.split_whitespace().collect();
    if toks.is_empty() {
        return Ok(Command::Noop);
    }

    let verb = toks[0];
    let args = &toks[1..];

    match verb {
        "set-seed" => parse_set_seed(args),
        "req" => parse_req(args),
        "show" => parse_show(args),
        "set-app-relays" => parse_url_list(args, "set-app-relays").map(Command::SetAppRelays),
        "set-indexer" => parse_url_list(args, "set-indexer").map(Command::SetIndexer),
        "set-dead" => parse_url_list(args, "set-dead").map(Command::SetDead),
        "set-budget" => parse_set_budget(args),
        "refresh" => parse_refresh(args),
        "expand" => parse_expand(args),
        "help" => Ok(Command::Help(args.first().map(|s| s.to_string()))),
        "quit" | "exit" => Ok(Command::Quit),
        other => Err(format!(
            "parse error: unknown verb '{other}' (try 'help')"
        )),
    }
}

fn parse_set_seed(args: &[&str]) -> Result<Command, String> {
    if args.len() != 1 {
        return Err("parse error: set-seed takes exactly one argument (nip05, npub, or hex)"
            .to_string());
    }
    let arg = args[0];
    let seed = if let Some(rest) = arg.strip_prefix("npub1") {
        // bech32 npubs are typically 59 chars total ("npub1" + 58 chars of data).
        if rest.is_empty() {
            return Err("parse error: empty npub".to_string());
        }
        SeedInput::Npub(arg.to_string())
    } else if arg.contains('@') {
        // basic NIP-05 shape: localpart@domain.tld
        let mut parts = arg.splitn(2, '@');
        let local = parts.next().unwrap_or("");
        let domain = parts.next().unwrap_or("");
        if local.is_empty() || domain.is_empty() || !domain.contains('.') {
            return Err(format!(
                "parse error: '{arg}' — expected nip05 'name@domain.tld'"
            ));
        }
        SeedInput::Nip05(arg.to_string())
    } else if arg.len() == 64 && arg.chars().all(|c| c.is_ascii_hexdigit()) {
        SeedInput::Hex(arg.to_lowercase())
    } else {
        return Err(format!(
            "parse error: '{arg}' — expected nip05 'name@domain', 'npub1…', or 64-hex pubkey"
        ));
    };
    Ok(Command::SetSeed(seed))
}

fn parse_req(args: &[&str]) -> Result<Command, String> {
    let mut filter = FilterAst::default();
    let mut have_any_field = false;
    let mut have_bare = false;

    for tok in args {
        let Some((key, val)) = tok.split_once('=') else {
            have_bare = true;
            continue;
        };
        have_any_field = true;
        parse_filter_field(&mut filter, key, val)?;
    }

    if have_bare && have_any_field {
        return Err(
            "parse error: req takes only key=value fields (no bare tokens)".to_string(),
        );
    }
    if have_bare && !have_any_field {
        return Err(
            "parse error: req requires key=value fields (e.g. 'kinds=1 authors=$follows')"
                .to_string(),
        );
    }
    if !have_any_field {
        return Err(
            "parse error: req requires at least one filter field (e.g. 'kinds=1')".to_string(),
        );
    }
    // "Missing required" check per §5.3.
    if filter.kinds.is_none() && filter.authors.is_none() && filter.ids.is_none() {
        return Err(
            "parse error: req requires at least one of 'kinds', 'authors', or 'ids'".to_string(),
        );
    }
    Ok(Command::Req(filter))
}

fn parse_filter_field(filter: &mut FilterAst, key: &str, val: &str) -> Result<(), String> {
    if val.is_empty() {
        return Err(format!("parse error: '{key}=' — at least one value required"));
    }
    match key {
        "kinds" => {
            let parts: Vec<&str> = val.split(',').collect();
            let mut out = Vec::with_capacity(parts.len());
            for p in parts {
                let n: u32 = p.parse().map_err(|_| {
                    format!("parse error: 'kinds={val}' — expected integer (got '{p}')")
                })?;
                out.push(n);
            }
            filter.kinds = Some(out);
        }
        "authors" => {
            filter.authors = Some(parse_value_list(val, key)?);
        }
        "ids" => {
            filter.ids = Some(parse_value_list(val, key)?);
        }
        "since" => {
            filter.since = Some(parse_timestamp(val, key)?);
        }
        "until" => {
            filter.until = Some(parse_timestamp(val, key)?);
        }
        "limit" => {
            let n: u32 = val.parse().map_err(|_| {
                format!("parse error: 'limit={val}' — expected non-negative integer")
            })?;
            filter.limit = Some(n);
        }
        k if k.starts_with('#') => {
            let letters: Vec<char> = k.chars().skip(1).collect();
            if letters.len() != 1 || !letters[0].is_ascii_alphabetic() {
                return Err(format!(
                    "parse error: '{k}={val}' — '#' filters take a single letter"
                ));
            }
            let letter = letters[0];
            let values = parse_value_list(val, k)?;
            // Multiple #X= fields for the same letter unite their values.
            filter.tags.entry(letter).or_default().extend(values);
        }
        other => {
            return Err(format!(
                "parse error: unknown field '{other}' (try 'help req')"
            ));
        }
    }
    Ok(())
}

fn parse_value_list(val: &str, key: &str) -> Result<Vec<Value>, String> {
    let parts: Vec<&str> = val.split(',').collect();
    let mut out = Vec::with_capacity(parts.len());
    for p in parts {
        if p.is_empty() {
            return Err(format!("parse error: '{key}={val}' — empty list element"));
        }
        validate_atom(p, key, val)?;
        if let Some(rest) = p.strip_prefix('$') {
            if rest.is_empty() {
                return Err(format!(
                    "parse error: '{key}={val}' — variable must have a name after '$'"
                ));
            }
            out.push(Value::Var(rest.to_string()));
        } else {
            out.push(Value::Lit(p.to_string()));
        }
    }
    Ok(out)
}

/// Accept a conservative atom alphabet (mirrors §5.2 regex):
/// `[A-Za-z0-9._:@/+-]+` for literals, or `$[A-Za-z_]+` for variables.
fn validate_atom(atom: &str, key: &str, val: &str) -> Result<(), String> {
    if atom.starts_with('$') {
        for c in atom.chars().skip(1) {
            if !(c.is_ascii_alphabetic() || c == '_') {
                return Err(format!(
                    "parse error: '{key}={val}' — variable name may only contain letters or '_' (got '{c}')"
                ));
            }
        }
        return Ok(());
    }
    for c in atom.chars() {
        if c.is_ascii_alphanumeric() {
            continue;
        }
        match c {
            '.' | '_' | ':' | '@' | '/' | '+' | '-' => continue,
            _ => {
                return Err(format!(
                    "parse error: '{key}={val}' — invalid character '{c}' in value"
                ));
            }
        }
    }
    Ok(())
}

fn parse_timestamp(val: &str, key: &str) -> Result<i64, String> {
    // Try unix ts first.
    if let Ok(n) = val.parse::<i64>() {
        return Ok(n);
    }
    // Then YYYY-MM-DD (UTC midnight).
    if let Ok(date) = chrono::NaiveDate::parse_from_str(val, "%Y-%m-%d") {
        if let Some(dt) = date.and_hms_opt(0, 0, 0) {
            return Ok(dt.and_utc().timestamp());
        }
    }
    Err(format!(
        "parse error: '{key}={val}' — expected YYYY-MM-DD or unix ts"
    ))
}

fn parse_show(args: &[&str]) -> Result<Command, String> {
    if args.is_empty() {
        return Ok(Command::Show(ShowTopic::State));
    }
    if args.len() != 1 {
        return Err("parse error: show takes one of {state, relays, budget, seen}".to_string());
    }
    let topic = match args[0] {
        "state" => ShowTopic::State,
        "relays" => ShowTopic::Relays,
        "budget" => ShowTopic::Budget,
        "seen" => ShowTopic::Seen,
        other => {
            return Err(format!(
                "parse error: unknown show topic '{other}' (try 'state', 'relays', 'budget', 'seen')"
            ));
        }
    };
    Ok(Command::Show(topic))
}

fn parse_url_list(args: &[&str], verb: &'static str) -> Result<Vec<String>, String> {
    if args.is_empty() {
        return Err(format!(
            "parse error: {verb} takes a comma-separated relay URL list"
        ));
    }
    if args.len() > 1 {
        return Err(format!(
            "parse error: {verb} expects a single comma-separated URL list (got {} tokens)",
            args.len()
        ));
    }
    let parts: Vec<String> = args[0]
        .split(',')
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect();
    if parts.is_empty() {
        return Err(format!("parse error: {verb} — empty URL list"));
    }
    for u in &parts {
        if !(u.starts_with("wss://") || u.starts_with("ws://")) {
            return Err(format!(
                "parse error: {verb} — '{u}' must start with 'wss://' or 'ws://'"
            ));
        }
    }
    Ok(parts)
}

fn parse_set_budget(args: &[&str]) -> Result<Command, String> {
    if args.is_empty() {
        return Err(
            "parse error: set-budget takes one or more of: max_connections=N, max_per_user=N, wall=Ns"
                .to_string(),
        );
    }
    let mut patch = BudgetPatch::default();
    for tok in args {
        let Some((key, val)) = tok.split_once('=') else {
            return Err(format!(
                "parse error: 'set-budget {tok}' — expected key=value"
            ));
        };
        match key {
            "max_connections" => {
                patch.max_connections = Some(val.parse().map_err(|_| {
                    format!("parse error: 'max_connections={val}' — expected positive integer")
                })?);
            }
            "max_per_user" => {
                patch.max_per_user = Some(val.parse().map_err(|_| {
                    format!("parse error: 'max_per_user={val}' — expected positive integer")
                })?);
            }
            "wall" => {
                patch.wall = Some(parse_duration(val)?);
            }
            other => {
                return Err(format!(
                    "parse error: unknown budget key '{other}' (try 'max_connections', 'max_per_user', 'wall')"
                ));
            }
        }
    }
    Ok(Command::SetBudget(patch))
}

fn parse_duration(val: &str) -> Result<Duration, String> {
    let (num_part, unit) = if let Some(stripped) = val.strip_suffix("ms") {
        (stripped, "ms")
    } else if let Some(stripped) = val.strip_suffix('s') {
        (stripped, "s")
    } else if let Some(stripped) = val.strip_suffix('m') {
        (stripped, "m")
    } else {
        // Bare integer → seconds.
        (val, "s")
    };
    let n: u64 = num_part
        .parse()
        .map_err(|_| format!("parse error: 'wall={val}' — expected duration like '20s', '500ms', '1m'"))?;
    Ok(match unit {
        "ms" => Duration::from_millis(n),
        "s" => Duration::from_secs(n),
        "m" => Duration::from_secs(n * 60),
        _ => unreachable!(),
    })
}

fn parse_refresh(args: &[&str]) -> Result<Command, String> {
    if args.is_empty() {
        return Ok(Command::Refresh(RefreshScope::All));
    }
    if args.len() != 1 {
        return Err(
            "parse error: refresh takes one of {follows, mailboxes, all} or no argument"
                .to_string(),
        );
    }
    let scope = match args[0] {
        "follows" => RefreshScope::Follows,
        "mailboxes" => RefreshScope::Mailboxes,
        "all" => RefreshScope::All,
        other => {
            return Err(format!(
                "parse error: unknown refresh scope '{other}' (try 'follows', 'mailboxes', 'all')"
            ));
        }
    };
    Ok(Command::Refresh(scope))
}

fn parse_expand(args: &[&str]) -> Result<Command, String> {
    if args.len() != 1 {
        return Err(
            "parse error: expand takes exactly one variable name (e.g. 'expand $follows')"
                .to_string(),
        );
    }
    let tok = args[0];
    let Some(name) = tok.strip_prefix('$') else {
        return Err(format!(
            "parse error: expand expects a $variable (got '{tok}')"
        ));
    };
    if name.is_empty() {
        return Err("parse error: expand — variable must have a name after '$'".to_string());
    }
    Ok(Command::Expand(VarName(name.to_string())))
}

// ── tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

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
}
