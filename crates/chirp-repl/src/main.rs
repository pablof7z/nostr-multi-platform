use std::io::{self, Write};

use chirp_repl::command::{parse, Command};
use chirp_repl::session::Session;

fn main() {
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("install rustls crypto provider");

    let mut session = Session::default();
    chirp_repl::render::banner();

    loop {
        print!("chirp-repl> ");
        let _ = io::stdout().flush();

        let mut line = String::new();
        match io::stdin().read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {}
            Err(e) => {
                chirp_repl::render::status_err(&format!("read failed: {e}"));
                break;
            }
        }

        let command = match parse(line.trim()) {
            Ok(Command::Noop) => continue,
            Ok(command) => command,
            Err(e) => {
                chirp_repl::render::status_err(&e);
                continue;
            }
        };

        match chirp_repl::actions::run(&mut session, command) {
            Ok(true) => break,
            Ok(false) => {}
            Err(e) => chirp_repl::render::status_err(&e),
        }
    }
}
