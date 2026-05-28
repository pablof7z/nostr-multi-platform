//! `nmp` — the NMP developer CLI.
//!
//! Two commands make NMP adoptable instead of hand-wired:
//!
//! * `nmp init <app-name>` — scaffold a new app (an `nmp.toml` manifest plus
//!   an `<app>-core` crate skeleton with a reactive view, an `ActionModule`,
//!   and a minimal headless shell stub).
//! * `nmp gen modules` — invoke the existing `nmp-codegen` pipeline to emit
//!   the per-app `nmp-app-<name>` FFI crate.
//! * `nmp add component <id>` — copy app-owned native source components from
//!   the offline NMP registry into an app tree.
//! * `nmp update component <id>` — refresh installed component sources from
//!   the registry while preserving locally edited files (conflict report).
//!
//! The scaffold compiles immediately after `nmp init`, and `nmp gen modules`
//! is deterministic. See `docs/cli.md`.

mod component;
mod doctor;
mod export;
mod gen;
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
        Some("gen") => gen::run(&args[1..]),
        Some("doctor") => doctor::run(&args[1..]),
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
        "      ./<app-name>) with an nmp.toml manifest and an <app-name>-core",
        "      crate skeleton (a reactive view, an ActionModule, plus a",
        "      headless shell stub). It compiles as-is.",
        "      --nmp-version writes versioned nmp-* dependencies for release",
        "      consumers; --nmp-path writes local checkout dependencies for",
        "      framework development.",
        "",
        "  nmp gen modules [--manifest nmp.toml] [--out DIR] [--check]",
        "      Generate the per-app nmp-app-<name> FFI crate from a manifest",
        "      via the nmp-codegen pipeline. --check verifies the on-disk",
        "      crate matches a fresh generation (deterministic codegen gate).",
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
        "      Move the app manifest to a pinned NMP release. Re-run",
        "      `nmp gen modules` after this command to refresh generated",
        "      crate dependencies.",
        "",
        "  nmp doctor [--manifest nmp.toml]",
        "      Validate the app's NMP dependency policy and report the release",
        "      or local checkout baseline used by codegen.",
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
