//! `nmp-repl` — interactive diagnostic REPL for the NMP planner + outbox.
//!
//! Read-only. No publishes, no AUTH, no NIP-77. v1 design lives in
//! `docs/design/nmp-repl.md`. The binary wires rustyline → parser →
//! command dispatch over a `Session`.

use std::borrow::Cow;

use rustyline::completion::{Completer, Pair};
use rustyline::error::ReadlineError;
use rustyline::highlight::Highlighter;
use rustyline::hint::Hinter;
use rustyline::validate::Validator;
use rustyline::{Context, Editor, Helper};

use nmp_repl::ast::Command;
use nmp_repl::commands;
use nmp_repl::parser::parse_line;
use nmp_repl::session::Session;

const VERBS: &[&str] = &[
    "set-seed",
    "req",
    "show",
    "set-app-relays",
    "set-indexer",
    "set-dead",
    "set-budget",
    "refresh",
    "expand",
    "help",
    // MLS / Marmot
    "create-account",
    "load-key",
    #[cfg(feature = "mls")]
    "mls-init",
    #[cfg(feature = "mls")]
    "mls-status",
    #[cfg(feature = "mls")]
    "mls-create",
    #[cfg(feature = "mls")]
    "mls-fetch-kp",
    #[cfg(feature = "mls")]
    "mls-invite",
    #[cfg(feature = "mls")]
    "mls-accept",
    #[cfg(feature = "mls")]
    "mls-send",
    #[cfg(feature = "mls")]
    "mls-messages",
    "quit",
    "exit",
];

const VARS: &[&str] = &["$me", "$seed", "$follows", "$relays", "$inbox"];

// ── rustyline helper: verb + variable completion ─────────────────────────────

struct ReplHelper;

impl Completer for ReplHelper {
    type Candidate = Pair;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        _ctx: &Context<'_>,
    ) -> rustyline::Result<(usize, Vec<Pair>)> {
        let upto = &line[..pos];

        // Column-0 verb completion: no whitespace yet on the line.
        if !upto.contains(char::is_whitespace) {
            let matches: Vec<Pair> = VERBS
                .iter()
                .filter(|v| v.starts_with(upto))
                .map(|v| Pair {
                    display: v.to_string(),
                    replacement: v.to_string(),
                })
                .collect();
            return Ok((0, matches));
        }

        // Variable completion: the current token starts with `$`.
        let token_start = upto
            .rfind(|c: char| c.is_whitespace() || c == ',' || c == '=')
            .map(|i| i + 1)
            .unwrap_or(0);
        let token = &upto[token_start..];
        if token.starts_with('$') {
            let matches: Vec<Pair> = VARS
                .iter()
                .filter(|v| v.starts_with(token))
                .map(|v| Pair {
                    display: v.to_string(),
                    replacement: v.to_string(),
                })
                .collect();
            return Ok((token_start, matches));
        }

        Ok((pos, Vec::new()))
    }
}

impl Hinter for ReplHelper {
    type Hint = String;
}

impl Highlighter for ReplHelper {
    fn highlight_prompt<'b, 's: 'b, 'p: 'b>(
        &'s self,
        prompt: &'p str,
        _default: bool,
    ) -> Cow<'b, str> {
        Cow::Borrowed(prompt)
    }
}

impl Validator for ReplHelper {}

impl Helper for ReplHelper {}

// ── flags ────────────────────────────────────────────────────────────────────

struct Flags {
    verbose: bool,
    json: bool,
}

fn parse_flags() -> Flags {
    let mut flags = Flags {
        verbose: false,
        json: false,
    };
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "-v" | "--verbose" => flags.verbose = true,
            "--json" => flags.json = true,
            "-h" | "--help" => {
                println!("nmp-repl — diagnostic REPL for the NMP planner + outbox");
                println!();
                println!("usage: nmp-repl [-v|--verbose] [--json]");
                println!();
                println!("  -v, --verbose   full relay URLs, extra timings");
                println!("  --json          one JSON line per relay state transition");
                std::process::exit(0);
            }
            other => {
                eprintln!("unknown flag '{other}' (try --help)");
                std::process::exit(2);
            }
        }
    }
    flags
}

// ── command dispatch ─────────────────────────────────────────────────────────

fn dispatch(session: &mut Session, cmd: Command) -> Result<bool, String> {
    match cmd {
        Command::Noop => Ok(false),
        Command::Quit => Ok(true),
        Command::Help(arg) => commands::help::run(arg).map(|_| false).map_err(|e| e.to_string()),
        Command::SetSeed(input) => commands::set_seed::run(session, input)
            .map(|_| false)
            .map_err(|e| e.to_string()),
        Command::Req(filter) => commands::req::run(session, filter)
            .map(|_| false)
            .map_err(|e| e.to_string()),
        Command::Show(topic) => commands::show::run(session, topic)
            .map(|_| false)
            .map_err(|e| e.to_string()),
        Command::SetAppRelays(urls) => commands::set_app_relays::run(session, urls)
            .map(|_| false)
            .map_err(|e| e.to_string()),
        Command::SetIndexer(urls) => commands::set_indexer::run(session, urls)
            .map(|_| false)
            .map_err(|e| e.to_string()),
        Command::SetDead(urls) => commands::set_dead::run(session, urls)
            .map(|_| false)
            .map_err(|e| e.to_string()),
        Command::SetBudget(patch) => commands::set_budget::run(session, patch)
            .map(|_| false)
            .map_err(|e| e.to_string()),
        Command::Refresh(scope) => commands::refresh::run(session, scope)
            .map(|_| false)
            .map_err(|e| e.to_string()),
        Command::Expand(var) => commands::expand::run(session, var)
            .map(|_| false)
            .map_err(|e| e.to_string()),
        Command::CreateAccount(name, relays) => commands::create_account::run(session, name, relays)
            .map(|_| false)
            .map_err(|e| e.to_string()),
        Command::LoadKey(input) => commands::load_key::run(session, input)
            .map(|_| false)
            .map_err(|e| e.to_string()),
        #[cfg(feature = "mls")]
        Command::MlsInit => commands::mls_init::run(session)
            .map(|_| false)
            .map_err(|e| e.to_string()),
        #[cfg(feature = "mls")]
        Command::MlsStatus => commands::mls_status::run(session)
            .map(|_| false)
            .map_err(|e| e.to_string()),
        #[cfg(feature = "mls")]
        Command::MlsCreate(name) => commands::mls_create::run(session, name)
            .map(|_| false)
            .map_err(|e| e.to_string()),
        #[cfg(feature = "mls")]
        Command::MlsFetchKp(npub) => commands::mls_fetch_kp::run(session, npub)
            .map(|_| false)
            .map_err(|e| e.to_string()),
        #[cfg(feature = "mls")]
        Command::MlsInvite(gid, npub) => commands::mls_invite::run(session, gid, npub)
            .map(|_| false)
            .map_err(|e| e.to_string()),
        #[cfg(feature = "mls")]
        Command::MlsAccept(arg) => commands::mls_accept::run(session, arg)
            .map(|_| false)
            .map_err(|e| e.to_string()),
        #[cfg(feature = "mls")]
        Command::MlsSend(gid, text) => commands::mls_send::run(session, gid, text)
            .map(|_| false)
            .map_err(|e| e.to_string()),
        #[cfg(feature = "mls")]
        Command::MlsMessages(gid) => commands::mls_messages::run(session, gid)
            .map(|_| false)
            .map_err(|e| e.to_string()),
    }
}

fn main() {
    // Pitfall §13.1: rustls provider MUST install exactly once at program
    // start, before any tungstenite::connect call.
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("install rustls crypto provider"); // doctrine-allow: D6 — process startup invariant; install failure means the binary cannot run

    let flags = parse_flags();

    let mut session = Session::new();
    session.verbose = flags.verbose;
    session.json = flags.json;

    let mut rl: Editor<ReplHelper, rustyline::history::DefaultHistory> =
        match Editor::new() {
            Ok(e) => e,
            Err(e) => {
                eprintln!("failed to init line editor: {e}");
                std::process::exit(1);
            }
        };
    rl.set_helper(Some(ReplHelper));

    // History file at ~/.cache/nmp-repl/history.
    let hist_path = dirs_cache_history();
    if let Some(p) = &hist_path {
        if let Some(parent) = p.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = rl.load_history(p);
    }

    if !flags.json {
        println!("nmp-repl v0.1 — diagnostic REPL (read-only). type 'help' or 'quit'.");
    }

    loop {
        let prompt = format!("nmp-repl[{}]> ", session.prompt_label());
        match rl.readline(&prompt) {
            Ok(line) => {
                let _ = rl.add_history_entry(line.as_str());
                let cmd = match parse_line(&line) {
                    Ok(c) => c,
                    Err(msg) => {
                        eprintln!("{msg}");
                        continue;
                    }
                };
                match dispatch(&mut session, cmd) {
                    Ok(true) => break,
                    Ok(false) => {}
                    Err(msg) => eprintln!("{msg}"),
                }
            }
            Err(ReadlineError::Interrupted) => {
                // Ctrl-C at the prompt: clear line, continue. (Mid-`req`
                // cancellation is the wall deadline's job — §12.)
                continue;
            }
            Err(ReadlineError::Eof) => {
                // Ctrl-D: clean exit.
                break;
            }
            Err(e) => {
                eprintln!("readline error: {e}");
                break;
            }
        }
    }

    if let Some(p) = &hist_path {
        let _ = rl.save_history(p);
    }
}

fn dirs_cache_history() -> Option<std::path::PathBuf> {
    // Minimal XDG-ish resolution without pulling the `dirs` crate.
    if let Ok(xdg) = std::env::var("XDG_CACHE_HOME") {
        if !xdg.is_empty() {
            return Some(std::path::PathBuf::from(xdg).join("nmp-repl").join("history"));
        }
    }
    let home = std::env::var("HOME").ok()?;
    Some(
        std::path::PathBuf::from(home)
            .join(".cache")
            .join("nmp-repl")
            .join("history"),
    )
}
