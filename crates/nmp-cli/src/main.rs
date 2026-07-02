//! `nmp` — the NMP developer CLI.
//!
//! The commands that make NMP adoptable instead of hand-wired:
//!
//! * `nmp init <app-name>` — scaffold a new app: a thin `<app>-core` crate
//!   with an explicit Rust composition root (ADR-0069), a `<app>-app`
//!   UniFFI facade, and one app-local typed action doorway.
//! * `nmp add component <id>` — copy app-owned native source components from
//!   the offline NMP registry into an app tree.
//! * `nmp update component <id>` — refresh installed component sources from
//!   the registry while preserving locally edited files (conflict report).
//! * `nmp upgrade` — bump the app's `nmp.toml` NMP dependency policy.
//!
//! The scaffold compiles immediately after `nmp init`. See `docs/cli.md`.

mod component;
mod crate_ownership;
mod export;
mod init;
mod manifest_edit;
mod upgrade;

use std::env;

fn main() {
    match run() {
        Ok(()) => {}
        Err(error) => {
            eprintln!("nmp: {error}");
            std::process::exit(1);
        }
    }
}

fn run() -> Result<(), String> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    match args.first().map(String::as_str) {
        Some("init") => init::run(&args[1..]),
        Some("crate-ownership") => crate_ownership::run(&args[1..]),
        Some("upgrade") => upgrade::run(&args[1..]),
        Some("add") => component::run_add(&args[1..]),
        Some("update") => component::run_update(&args[1..]),
        Some("export") => match args.get(1).map(String::as_str) {
            Some("jsrepo") => export::run(&args[2..]),
            Some(other) => Err(format!("unknown export target `{other}`; try `jsrepo`")),
            None => Err("usage: nmp export <target>  (e.g. jsrepo)".to_string()),
        },
        Some("--help") | Some("-h") | Some("help") | None => {
            println!("{}", help());
            Ok(())
        }
        Some(other) => Err(format!("unknown command `{other}`\n\n{}", help())),
    }
}

fn help() -> String {
    [
        "usage:",
        "  nmp init <app-name> [--path DIR] [--nmp-version VERSION | --nmp-path DIR]",
        "      Scaffold a new NMP app. Creates a workspace at DIR (default",
        "      ./<app-name>) with an nmp.toml manifest, an <app-name>-core",
        "      crate with an explicit Rust composition root, an <app-name>-app",
        "      UniFFI facade, and one generated app-local action builder.",
        "      --nmp-version pins the nmp-* git dependencies to a release rev for",
        "      release consumers; --nmp-path writes local checkout dependencies",
        "      for framework development.",
        "",
        "  nmp add component <id> [--path DIR] [--registry DIR] [--with ROLES]",
        "      Copy app-owned source components from the local offline registry",
        "      into DIR (default current directory) and update nmp.components.lock.",
        "",
        "  nmp update component <id> [--path DIR] [--registry DIR]",
        "      Refresh an installed component's sources from the registry.",
        "      Files that match their lock baseline are overwritten and the lock",
        "      hash + version are refreshed. Files with local edits are reported",
        "      as conflicts and left untouched.",
        "",
        "  nmp upgrade --to VERSION [--manifest nmp.toml]",
        "      Move the app manifest to a pinned NMP release and rewrite the",
        "      app crate's nmp-* dependencies to that version. Prints the",
        "      target release migration note.",
        "",
        "  nmp crate-ownership [--format human|tsv|json]",
        "      Report positive ownership descriptors declared by active workspace",
        "      crates. Use `audit --deny` to fail on missing descriptors or",
        "      duplicate exclusive ownership scopes.",
        "",
        "  nmp export jsrepo [--output DIR] [--registry DIR]",
        "      Emit a jsrepo/shadcn-compatible registry.json (full index) plus",
        "      per-component r/<slug>.json files into DIR (default current",
        "      directory). File content is inlined so consumers need no extra",
        "      requests. Commit the output to web/registry/public/ to serve",
        "      the live registry at https://nmpui.f7z.io.",
    ]
    .join("\n")
}

mod ownership;
