//! `nmp.blossom.upload` — the Blossom BUD-02 upload [`ActionModule`].
//!
//! `start` validates the upload intent; `execute` enqueues
//! `ActorCommand::Protocol(BlossomUploadCommand{...})` and returns immediately
//! (D8 — no hashing / HTTP on the dispatch path). The protocol command owns the
//! full Build → Sign → Transport pipeline (see [`crate::upload`]).
//!
//! App-facing contract: dispatch `nmp.blossom.upload`, read the blob descriptor
//! from `action_results[correlation_id].result`. No HTTP, base64, header
//! construction, or sign-for-return in app code.

use nmp_core::substrate::{ActionContext, ActionModule, ActionRejection};
use nmp_core::ActorCommand;
use serde::{Deserialize, Serialize};

use crate::upload::BlossomUploadCommand;

/// Wire shape for `nmp.blossom.upload` — the JSON body a host passes to
/// `nmp_app_dispatch_action`.
///
/// ```json
/// {
///   "file_path": "/var/mobile/.../avatar.png",
///   "content_type": "image/png",
///   "servers": ["https://blossom.primal.net"],
///   "signer_pubkey": "<hex-or-omitted>"
/// }
/// ```
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct UploadInput {
    /// Local path to the blob the app already wrote. Required, non-empty.
    pub file_path: String,
    /// MIME type. Optional — sniffed from the file extension when omitted.
    #[serde(default)]
    pub content_type: Option<String>,
    /// BUD-02 blob-server base URLs. Required, non-empty; each must be a valid
    /// http(s) URL.
    #[serde(default)]
    pub servers: Vec<String>,
    /// Account selector for the kind:24242 signature. `None`/omitted = active
    /// account; `Some(hex)` = a named roster key (per-podcast NIP-F4). Local vs
    /// bunker is transparent.
    #[serde(default)]
    pub signer_pubkey: Option<String>,
}

/// The `nmp.blossom.upload` [`ActionModule`].
pub struct UploadAction;

impl ActionModule for UploadAction {
    const NAMESPACE: &'static str = "nmp.blossom.upload";
    type Action = UploadInput;

    /// Validate the upload request. Rejects:
    /// - empty `file_path`
    /// - empty `servers`
    /// - any server that is not a valid `http(s)://` URL
    fn start(&self, _ctx: &mut ActionContext, action: Self::Action) -> Result<(), ActionRejection> {
        if action.file_path.trim().is_empty() {
            return Err(ActionRejection::Invalid(
                "blossom upload requires a non-empty file_path".into(),
            ));
        }
        let servers: Vec<&String> = action
            .servers
            .iter()
            .filter(|s| !s.trim().is_empty())
            .collect();
        if servers.is_empty() {
            return Err(ActionRejection::Invalid(
                "blossom upload requires at least one server".into(),
            ));
        }
        for server in servers {
            if !is_http_url(server) {
                return Err(ActionRejection::Invalid(format!(
                    "blossom server must be an http(s) URL: {server}"
                )));
            }
        }
        Ok(())
    }

    /// Settles asynchronously — `execute` enqueues the protocol command and
    /// returns; the worker posts the terminal (`RecordActionSuccess` with the
    /// descriptor, or `RecordActionFailure`) against `correlation_id`.
    fn is_async_completing() -> bool {
        true
    }

    /// Enqueue `ActorCommand::Protocol(BlossomUploadCommand{...})` carrying the
    /// validated upload intent. Returns immediately (D8).
    fn execute(
        &self,
        action: Self::Action,
        correlation_id: &str,
        send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        let servers: Vec<String> = action
            .servers
            .into_iter()
            .filter(|s| !s.trim().is_empty())
            .collect();
        send(ActorCommand::Protocol(Box::new(BlossomUploadCommand {
            file_path: action.file_path,
            content_type: action.content_type,
            servers,
            signer_pubkey: action.signer_pubkey,
            correlation_id: correlation_id.to_string(),
        })));
        Ok(())
    }
}

/// True for a syntactically-plausible `http://` / `https://` URL with a host.
fn is_http_url(s: &str) -> bool {
    let rest = s
        .strip_prefix("https://")
        .or_else(|| s.strip_prefix("http://"));
    match rest {
        Some(after) => !after.trim().is_empty() && !after.starts_with('/'),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn ctx() -> ActionContext {
        ActionContext::default()
    }

    fn well_formed() -> UploadInput {
        UploadInput {
            file_path: "/tmp/avatar.png".to_string(),
            content_type: Some("image/png".to_string()),
            servers: vec!["https://blossom.example".to_string()],
            signer_pubkey: None,
        }
    }

    fn run_execute(input: UploadInput) -> Vec<ActorCommand> {
        let captured: RefCell<Vec<ActorCommand>> = RefCell::new(Vec::new());
        UploadAction.execute(input, "cid-blossom", &|cmd| captured.borrow_mut().push(cmd))
            .expect("execute succeeds");
        captured.into_inner()
    }

    #[test]
    fn namespace_is_nmp_blossom_upload() {
        assert_eq!(UploadAction::NAMESPACE, "nmp.blossom.upload");
    }

    #[test]
    fn is_async_completing_is_true() {
        assert!(UploadAction::is_async_completing());
    }

    #[test]
    fn start_accepts_well_formed_input() {
        assert!(UploadAction.start(&mut ctx(), well_formed()).is_ok());
    }

    #[test]
    fn start_rejects_empty_file_path() {
        let input = UploadInput {
            file_path: "   ".to_string(),
            ..well_formed()
        };
        assert!(matches!(
            UploadAction.start(&mut ctx(), input),
            Err(ActionRejection::Invalid(_))
        ));
    }

    #[test]
    fn start_rejects_empty_servers() {
        let input = UploadInput {
            servers: vec![],
            ..well_formed()
        };
        assert!(matches!(
            UploadAction.start(&mut ctx(), input),
            Err(ActionRejection::Invalid(_))
        ));
    }

    #[test]
    fn start_rejects_non_http_server() {
        let input = UploadInput {
            servers: vec!["ftp://nope.example".to_string()],
            ..well_formed()
        };
        assert!(matches!(
            UploadAction.start(&mut ctx(), input),
            Err(ActionRejection::Invalid(_))
        ));
    }

    #[test]
    fn execute_emits_protocol_blossom_upload_command() {
        let cmds = run_execute(well_formed());
        assert_eq!(cmds.len(), 1, "exactly one command: {cmds:?}");
        let ActorCommand::Protocol(boxed) = &cmds[0] else {
            panic!("expected ActorCommand::Protocol(...), got {:?}", cmds[0]);
        };
        let dbg = format!("{boxed:?}");
        assert!(dbg.contains("BlossomUploadCommand"), "{dbg}");
        assert!(dbg.contains("/tmp/avatar.png"), "file_path surfaces: {dbg}");
        assert!(dbg.contains("blossom.example"), "server surfaces: {dbg}");
        assert!(
            dbg.contains("cid-blossom"),
            "correlation_id surfaces: {dbg}"
        );
    }

    #[test]
    fn is_http_url_accepts_https_and_http_rejects_others() {
        assert!(is_http_url("https://b.example"));
        assert!(is_http_url("http://b.example:3000/base"));
        assert!(!is_http_url("ftp://b.example"));
        assert!(!is_http_url("b.example"));
        assert!(!is_http_url("https://"));
    }
}
