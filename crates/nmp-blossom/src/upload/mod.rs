//! `BlossomUploadCommand` — the BUD-02 upload [`ProtocolCommand`].
//!
//! Dispatched as `ActorCommand::Protocol(Box::new(BlossomUploadCommand{...}))`
//! by [`crate::action::UploadAction::execute`]. The full Build → Sign →
//! Transport pipeline lives here; `nmp-core` only routes the boxed command, the
//! generic `SignEventForAccount` sign port, and the generic action-result
//! terminals (D0 — no Blossom noun, no HTTP crate in the kernel).
//!
//! # Flow (ADR-0043 "Internal flow")
//!
//! ```text
//! run(ctx)                                          [actor thread]
//!   record Requested stage; capture created_at = ctx.now_secs()  (D7)
//!   spawn worker A:                                 [worker thread]
//!     stream file → sha256 (x tag) + size + content_type
//!     build kind:24242 (auth.rs, created_at injected)            (D8)
//!     send SignEventForAccount{ unsigned, signer_pubkey, continuation }
//!   kernel signs (active or named; local inline / bunker parked) [actor thread]
//!     → continuation(Ok(signed) | Err(reason))
//!   continuation spawns worker B:                   [worker thread]
//!     base64(signed) → Authorization: Nostr header
//!     PUT blob to EACH server; aggregate (ok if ≥1 accepts)      (D8)
//!     RecordActionSuccess{cid, result_json} | RecordActionFailure{cid, reason}
//! ```
//!
//! D7 — the kernel owns the wall clock: `created_at` is captured from
//! `ctx.now_secs()` on the actor thread and moved into the worker; the worker
//! NEVER calls `SystemTime::now`. D8 — all file hashing and HTTP run on spawned
//! `std::thread`s, never the actor thread. D13 — the worker holds a
//! `SignedEvent`, never raw key bytes (the sign port returns a signed event).

pub mod http;

use std::io::Read;

use nmp_core::substrate::{
    build_sign_event_for_account, ProtocolCommand, ProtocolCommandContext, ProtocolCommandError,
};
use nmp_core::ActorCommand;
use sha2::{Digest, Sha256};

use crate::auth;
use http::BlobDescriptor;

/// Bytes read from the blob file per chunk while streaming through the hasher.
const HASH_CHUNK_BYTES: usize = 64 * 1024;

/// The BUD-02 upload [`ProtocolCommand`]. Carries the validated upload intent
/// from `UploadAction::execute`; every field is consumed in [`Self::run`].
#[derive(Debug)]
pub struct BlossomUploadCommand {
    /// Local filesystem path to the blob the app already wrote.
    pub file_path: String,
    /// MIME type. `None` → sniffed from the file extension (default
    /// `application/octet-stream`).
    pub content_type: Option<String>,
    /// BUD-02 blob-server base URLs. Non-empty (validated in `start`).
    pub servers: Vec<String>,
    /// Account selector for the kind:24242 signature. `None` = active account;
    /// `Some(hex)` = a named roster key. Local-vs-bunker is transparent.
    pub signer_pubkey: Option<String>,
    /// Registry-minted action correlation id — the `action_results` key the
    /// host's spinner is waiting on.
    pub correlation_id: String,
}

impl ProtocolCommand for BlossomUploadCommand {
    fn run(
        self: Box<Self>,
        ctx: &mut ProtocolCommandContext<'_>,
    ) -> Result<(), ProtocolCommandError> {
        let Self {
            file_path,
            content_type,
            servers,
            signer_pubkey,
            correlation_id,
        } = *self;

        // Track the `Requested` stage so the host stage observer sees the
        // transition before the worker posts the terminal.
        ctx.record_action_stage_requested(&correlation_id);

        // D7 — capture the kernel wall clock HERE, on the actor thread, and move
        // it into the worker. The worker must NOT call SystemTime::now.
        let created_at = ctx.now_secs();

        // The worker needs its own command sender to post the sign command and
        // (later) the terminal. D8 — everything below the spawn runs off-actor.
        let worker_tx = ctx.command_sender_clone();

        std::thread::spawn(move || {
            run_upload_worker(
                file_path,
                content_type,
                servers,
                signer_pubkey,
                correlation_id,
                created_at,
                worker_tx,
            );
        });

        Ok(())
    }
}

/// Worker A: stream the file → sha256 + size, build the kind:24242, and send the
/// `SignEventForAccount` command. The continuation it carries spawns worker B.
/// All blocking work (file I/O, hashing, HTTP) is on worker threads (D8).
fn run_upload_worker(
    file_path: String,
    content_type: Option<String>,
    servers: Vec<String>,
    signer_pubkey: Option<String>,
    correlation_id: String,
    created_at: u64,
    worker_tx: nmp_core::CommandSender,
) {
    // Stream the file through the hasher (never load the whole blob into a
    // single buffer for hashing).
    let (sha256_hex, size) = match hash_file_streaming(&file_path) {
        Ok(v) => v,
        Err(reason) => {
            fail(&worker_tx, correlation_id, format!("read blob: {reason}"));
            return;
        }
    };
    let resolved_content_type = content_type.unwrap_or_else(|| sniff_content_type(&file_path));

    // Build the unsigned kind:24242 with the streamed sha256 (D7 created_at).
    let unsigned = auth::build_upload_auth(&sha256_hex, created_at);

    // Read the blob body for the PUT leg. (A future streaming-body seam can
    // avoid the second read; v1 reads the bytes once for the PUT.)
    let body = match std::fs::read(&file_path) {
        Ok(b) => b,
        Err(e) => {
            fail(&worker_tx, correlation_id, format!("read blob body: {e}"));
            return;
        }
    };

    // The continuation runs on the actor thread (inline for local, from the
    // idle-loop drain for bunker) and spawns worker B with the signed event.
    let cont_tx = worker_tx.clone();
    let command = build_sign_event_for_account(unsigned, signer_pubkey, move |signed_result| {
        match signed_result {
            Ok(signed) => {
                spawn_put_worker(
                    cont_tx,
                    servers,
                    resolved_content_type,
                    sha256_hex,
                    size,
                    body,
                    correlation_id,
                    signed,
                );
            }
            Err(reason) => {
                fail(
                    &cont_tx,
                    correlation_id,
                    format!("sign upload authorization: {reason}"),
                );
            }
        }
    });
    // The sign command must run on the actor thread; post it through the
    // worker's sender. A disconnected channel (post-teardown) is benign (D6).
    let _ = worker_tx.send(command);
}

/// Worker B: build the `Authorization` header, PUT the blob to EACH server,
/// aggregate, and post the action terminal. Spawned by the sign continuation so
/// the actor thread is never blocked on HTTP (D8).
#[allow(clippy::too_many_arguments)]
fn spawn_put_worker(
    worker_tx: nmp_core::CommandSender,
    servers: Vec<String>,
    content_type: String,
    sha256_hex: String,
    size: u64,
    body: Vec<u8>,
    correlation_id: String,
    signed: nmp_core::substrate::SignedEvent,
) {
    std::thread::spawn(move || {
        let signed_json = signed.to_nip01_json();
        let auth_header = auth::authorization_header_value(&signed_json);

        let mut per_server: Vec<ServerOutcome> = Vec::with_capacity(servers.len());
        for server in &servers {
            match http::put_blob(server, &auth_header, &content_type, body.clone()) {
                Ok(descriptor) => per_server.push(ServerOutcome::ok(server, descriptor)),
                Err(reason) => per_server.push(ServerOutcome::err(server, reason)),
            }
        }

        match aggregate(&sha256_hex, size, &content_type, &per_server) {
            Ok(result_json) => {
                let _ = worker_tx.send(ActorCommand::RecordActionSuccess {
                    correlation_id,
                    result_json: Some(result_json),
                });
            }
            Err(reason) => {
                fail(&worker_tx, correlation_id, reason);
            }
        }
    });
}

/// One server's upload outcome, used for multi-server aggregation.
struct ServerOutcome {
    server: String,
    result: Result<BlobDescriptor, String>,
}

impl ServerOutcome {
    fn ok(server: &str, descriptor: BlobDescriptor) -> Self {
        Self {
            server: server.to_string(),
            result: Ok(descriptor),
        }
    }
    fn err(server: &str, reason: String) -> Self {
        Self {
            server: server.to_string(),
            result: Err(reason),
        }
    }
}

/// Aggregate per-server outcomes into the Decision-4 JSON (the `result_json`
/// the host reads from `action_results[cid].result`).
///
/// * Single server → a flat BUD-02 descriptor.
/// * Multiple servers → `{ sha256, size, type, uploaded, servers: [...] }`,
///   each entry `{ server, ok, url? , error? }`.
///
/// `Ok` iff at least one server accepted. All-fail → `Err(aggregated reason)`
/// so the action records a `RecordActionFailure` and the host spinner clears
/// (D6).
fn aggregate(
    sha256_hex: &str,
    size: u64,
    content_type: &str,
    per_server: &[ServerOutcome],
) -> Result<String, String> {
    let accepted: Vec<&ServerOutcome> = per_server.iter().filter(|o| o.result.is_ok()).collect();
    if accepted.is_empty() {
        let reasons = per_server
            .iter()
            .map(|o| {
                let reason = o.result.as_ref().err().map(String::as_str).unwrap_or("");
                format!("{}: {reason}", o.server)
            })
            .collect::<Vec<_>>()
            .join("; ");
        return Err(format!(
            "all {} server(s) rejected the upload: {reasons}",
            per_server.len()
        ));
    }

    // Single-server success → the flat descriptor itself.
    if per_server.len() == 1 {
        let descriptor = per_server[0]
            .result
            .as_ref()
            .expect("accepted is non-empty and len==1");
        return serde_json::to_string(descriptor).map_err(|e| format!("serialize descriptor: {e}"));
    }

    // Multi-server → aggregated shape with per-server itemisation. Use the
    // `uploaded` of the first accepting server (any accepted descriptor's
    // metadata is authoritative for sha256/size).
    let uploaded = accepted
        .iter()
        .find_map(|o| o.result.as_ref().ok().map(|d| d.uploaded))
        .unwrap_or(0);
    let servers_json: Vec<serde_json::Value> = per_server
        .iter()
        .map(|o| match &o.result {
            Ok(d) => serde_json::json!({ "server": o.server, "ok": true, "url": d.url }),
            Err(e) => serde_json::json!({ "server": o.server, "ok": false, "error": e }),
        })
        .collect();
    let value = serde_json::json!({
        "sha256": sha256_hex,
        "size": size,
        "type": content_type,
        "uploaded": uploaded,
        "servers": servers_json,
    });
    serde_json::to_string(&value).map_err(|e| format!("serialize aggregate: {e}"))
}

/// Record a terminal `RecordActionFailure` so the host spinner clears (D6).
fn fail(worker_tx: &nmp_core::CommandSender, correlation_id: String, reason: String) {
    let _ = worker_tx.send(ActorCommand::RecordActionFailure {
        correlation_id,
        reason,
    });
}

/// Stream a file through SHA-256, returning `(lowercase-hex digest, size)`.
/// Bounded memory: reads in [`HASH_CHUNK_BYTES`] chunks.
fn hash_file_streaming(path: &str) -> Result<(String, u64), String> {
    let mut file = std::fs::File::open(path).map_err(|e| format!("open {path}: {e}"))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; HASH_CHUNK_BYTES];
    let mut size: u64 = 0;
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|e| format!("read {path}: {e}"))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        size += n as u64;
    }
    let digest = hasher.finalize();
    let hex = digest
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    Ok((hex, size))
}

/// Best-effort MIME sniff from the file extension. Defaults to
/// `application/octet-stream` for unknown / missing extensions.
fn sniff_content_type(path: &str) -> String {
    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "mp3" => "audio/mpeg",
        "m4a" => "audio/mp4",
        "wav" => "audio/wav",
        "mp4" => "video/mp4",
        "pdf" => "application/pdf",
        "txt" => "text/plain",
        "json" => "application/json",
        _ => "application/octet-stream",
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn descriptor(url: &str, uploaded: u64) -> BlobDescriptor {
        BlobDescriptor {
            url: url.to_string(),
            sha256: "abc".to_string(),
            size: 5,
            mime_type: Some("image/png".to_string()),
            uploaded,
        }
    }

    #[test]
    fn sniff_content_type_maps_known_extensions() {
        assert_eq!(sniff_content_type("/a/b/x.png"), "image/png");
        assert_eq!(sniff_content_type("/a/b/x.mp3"), "audio/mpeg");
        assert_eq!(sniff_content_type("/a/b/x"), "application/octet-stream");
        assert_eq!(
            sniff_content_type("/a/b/x.unknown"),
            "application/octet-stream"
        );
    }

    #[test]
    fn hash_file_streaming_computes_real_sha256_and_size() {
        // Known vector: sha256("hello") = 2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824
        let dir = std::env::temp_dir();
        let path = dir.join(format!("nmp-blossom-hash-{}.bin", std::process::id()));
        std::fs::write(&path, b"hello").unwrap();
        let (hex, size) = hash_file_streaming(path.to_str().unwrap()).unwrap();
        assert_eq!(
            hex,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
        assert_eq!(size, 5);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn aggregate_single_server_success_is_flat_descriptor() {
        let outcomes = vec![ServerOutcome::ok(
            "https://b1.example",
            descriptor("https://b1.example/abc.png", 1733356800),
        )];
        let json = aggregate("abc", 5, "image/png", &outcomes).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["sha256"], "abc");
        assert_eq!(v["url"], "https://b1.example/abc.png");
        assert!(
            v.get("servers").is_none(),
            "single-server is flat, no servers[]"
        );
    }

    #[test]
    fn aggregate_multi_server_partial_is_ok_with_itemised_servers() {
        let outcomes = vec![
            ServerOutcome::ok(
                "https://b1.example",
                descriptor("https://b1.example/abc.png", 1733356800),
            ),
            ServerOutcome::err("https://b2.example", "413 Payload Too Large".to_string()),
        ];
        let json = aggregate("abc", 5, "image/png", &outcomes).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["sha256"], "abc");
        let servers = v["servers"].as_array().unwrap();
        assert_eq!(servers.len(), 2);
        assert_eq!(servers[0]["ok"], true);
        assert_eq!(servers[0]["url"], "https://b1.example/abc.png");
        assert_eq!(servers[1]["ok"], false);
        assert!(servers[1]["error"].as_str().unwrap().contains("413"));
    }

    #[test]
    fn aggregate_all_fail_is_err_with_aggregated_reason() {
        let outcomes = vec![
            ServerOutcome::err("https://b1.example", "500 boom".to_string()),
            ServerOutcome::err("https://b2.example", "413 too big".to_string()),
        ];
        let err = aggregate("abc", 5, "image/png", &outcomes).expect_err("all fail");
        assert!(err.contains("all 2 server(s) rejected"), "{err}");
        assert!(
            err.contains("b1.example") && err.contains("b2.example"),
            "{err}"
        );
    }
}
