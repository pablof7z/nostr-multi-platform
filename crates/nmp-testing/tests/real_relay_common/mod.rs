//! Shared harness for the `real_relay_*` honest-validation suite.
//!
//! These helpers open **real** websockets to public Nostr relays. They are
//! consumed by the per-scenario integration tests (`real_relay_connect`,
//! `real_relay_outbox`, `real_relay_nip77`, `real_relay_nip42`,
//! `real_relay_replan`) and the soak runner (`real_relay_soak`). Every test
//! that uses this module is `#[ignore]`-gated so `cargo test --workspace`
//! stays hermetic; run explicitly with `-- --ignored --nocapture`.
//!
//! Doctrine: this module is a *consumer only* of `nmp-core` / `nmp-nip*`
//! public surfaces (D0 — no app nouns leak in here; we touch only protocol
//! primitives and the in-memory store).
//!
//! ## Honest-validation contract
//!
//! When a relay is unreachable, refuses, or simply does not exhibit the
//! behaviour a scenario needs within budget, the scenario must **report the
//! gap loudly** (via [`write_report`]) and SKIP — never fabricate a green
//! result. A skipped scenario with a written finding is the point of this
//! suite; a faked pass defeats it.
//!
//! Files pull this in with:
//! ```ignore
//! #[path = "real_relay_common/mod.rs"]
//! mod common;
//! ```

#![allow(dead_code)]

use std::fs;
use std::io::Write as _;
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::Once;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tungstenite::stream::MaybeTlsStream;
use tungstenite::{connect, Message, WebSocket};

pub type RelaySocket = WebSocket<MaybeTlsStream<TcpStream>>;

/// Public relays used across scenarios. Reachability is never assumed —
/// callers go through [`try_open`] / [`open_with_timeout`] and SKIP on miss.
pub const DAMUS_RELAY: &str = "wss://relay.damus.io";
pub const NOS_LOL: &str = "wss://nos.lol";
pub const PRIMAL_RELAY: &str = "wss://relay.primal.net";
pub const NOSTR_BAND: &str = "wss://relay.nostr.band";

/// Per-`read()` socket timeout. Short so drain loops stay responsive to a
/// wall-clock deadline.
pub const READ_TIMEOUT: Duration = Duration::from_millis(250);
/// Default ceiling for a single connect attempt.
pub const CONNECT_BUDGET: Duration = Duration::from_secs(8);

/// One-time TLS provider install (mirrors `relay_worker::install_rustls_provider`).
pub fn install_rustls_provider() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

fn apply_read_timeout(socket: &mut RelaySocket) {
    match socket.get_mut() {
        MaybeTlsStream::Plain(s) => {
            let _ = s.set_read_timeout(Some(READ_TIMEOUT));
        }
        MaybeTlsStream::Rustls(s) => {
            let _ = s.get_ref().set_read_timeout(Some(READ_TIMEOUT));
        }
        #[allow(unreachable_patterns)]
        _ => {}
    }
}

/// Blocking connect with a per-read timeout applied. Prefer
/// [`open_with_timeout`] in long-running contexts (soak) so a hung TLS
/// handshake cannot stall the whole run.
pub fn open(url: &str) -> Result<RelaySocket, String> {
    install_rustls_provider();
    let (mut socket, _response) = connect(url).map_err(|e| e.to_string())?;
    apply_read_timeout(&mut socket);
    Ok(socket)
}

/// Connect on a worker thread and abandon it if the handshake exceeds
/// `budget`. Returns `Err` with a diagnostic on timeout / failure.
pub fn open_with_timeout(url: &str, budget: Duration) -> Result<RelaySocket, String> {
    install_rustls_provider();
    let (tx, rx) = mpsc::channel();
    let url_owned = url.to_string();
    thread::spawn(move || {
        let result = connect(&url_owned)
            .map_err(|e| e.to_string())
            .map(|(mut socket, _)| {
                apply_read_timeout(&mut socket);
                socket
            });
        // Receiver may be gone if we already timed out — that's fine.
        let _ = tx.send(result);
    });
    match rx.recv_timeout(budget) {
        Ok(inner) => inner,
        Err(_) => Err(format!("connect to {url} exceeded {budget:?}")),
    }
}

/// Best-effort open: on any failure, print a uniform `SKIP:` line and return
/// `None` so the scenario can record a finding and bail cleanly.
pub fn try_open(url: &str) -> Option<RelaySocket> {
    match open_with_timeout(url, CONNECT_BUDGET) {
        Ok(s) => Some(s),
        Err(e) => {
            eprintln!("SKIP: cannot reach {url}: {e}");
            None
        }
    }
}

pub fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

pub fn now_s() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Build, sign, and serialize a kind:1 note via the `nostr` crate. Returns
/// `(event_id_hex, author_pubkey_hex, event_json)` ready to wrap in an
/// `["EVENT", <json>]` envelope. Each call uses a fresh ephemeral key.
pub fn build_kind1(content: &str) -> (String, String, String) {
    use nostr::util::JsonUtil as _;
    use nostr::{EventBuilder, Keys};
    let keys = Keys::generate();
    let event = EventBuilder::text_note(content)
        .sign_with_keys(&keys)
        .expect("sign kind:1");
    (event.id.to_hex(), event.pubkey.to_hex(), event.as_json())
}

pub fn send_text(socket: &mut RelaySocket, text: impl Into<String>) -> Result<(), String> {
    socket
        .send(Message::Text(text.into()))
        .map_err(|e| e.to_string())
}

/// Drain text frames until `pred` returns `true` or `deadline` passes.
/// Returns `true` if the predicate was satisfied. Non-text frames and
/// would-block timeouts are ignored; a hard socket error stops the loop.
pub fn drain_until(
    socket: &mut RelaySocket,
    deadline: Instant,
    mut pred: impl FnMut(&str) -> bool,
) -> bool {
    while Instant::now() < deadline {
        match socket.read() {
            Ok(Message::Text(text)) => {
                if pred(&text) {
                    return true;
                }
            }
            Ok(_) => {}
            Err(tungstenite::Error::Io(e))
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(e) => {
                eprintln!("socket error during drain: {e}");
                return false;
            }
        }
    }
    false
}

/// Absolute path to `docs/perf/real-relay/` (resolved from the crate
/// manifest dir so it works regardless of test CWD).
pub fn report_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/perf/real-relay")
        .canonicalize()
        .unwrap_or_else(|_| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/perf/real-relay")
        })
}

/// Write a single-page markdown report/finding. `slug` becomes the filename
/// stem (`<slug>.md`). Always writes — a green run must leave evidence too.
pub fn write_report(slug: &str, body: &str) {
    let dir = report_dir();
    if let Err(e) = fs::create_dir_all(&dir) {
        eprintln!("WARN: could not create {}: {e}", dir.display());
        return;
    }
    let path = dir.join(format!("{slug}.md"));
    match fs::File::create(&path) {
        Ok(mut f) => {
            if let Err(e) = f.write_all(body.as_bytes()) {
                eprintln!("WARN: could not write {}: {e}", path.display());
            } else {
                eprintln!("REPORT: wrote {}", path.display());
            }
        }
        Err(e) => eprintln!("WARN: could not create {}: {e}", path.display()),
    }
}

/// Convenience: a `## Verdict` line is `PASS` / `SKIP` / `FAIL` so reports
/// are greppable across the whole `docs/perf/real-relay/` directory.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Verdict {
    Pass,
    Skip,
    Fail,
}

impl Verdict {
    pub fn as_str(self) -> &'static str {
        match self {
            Verdict::Pass => "PASS",
            Verdict::Skip => "SKIP",
            Verdict::Fail => "FAIL",
        }
    }
}

/// Render a uniform report page. Keeps every finding one-page and greppable.
pub fn report_page(
    title: &str,
    scenario: &str,
    verdict: Verdict,
    relays: &[&str],
    body_md: &str,
) -> String {
    format!(
        "---\nscenario: {scenario}\nverdict: {verdict}\ngenerated_at: {ts}\nrelays: [{relays}]\n---\n\n# {title}\n\n## Verdict: {verdict}\n\n{body}\n",
        scenario = scenario,
        verdict = verdict.as_str(),
        ts = now_s(),
        relays = relays
            .iter()
            .map(|r| format!("\"{r}\""))
            .collect::<Vec<_>>()
            .join(", "),
        title = title,
        body = body_md,
    )
}
