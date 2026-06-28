use serde_json::{Map, Value};

use crate::app::{AppRuntime, AppState};

pub fn execute(input: &str, state: &mut AppState, runtime: &AppRuntime) {
    let command = input.trim();
    if command.is_empty() {
        state.status = "command is empty".to_string();
        return;
    }

    let result = match first_word(command) {
        ("help", _) => help_text(),
        ("account", rest) => account(rest, runtime),
        ("profile", rest) => profile(rest, runtime),
        ("relay", rest) => relay(rest, runtime),
        ("dm-relays", rest) => dm_relays(rest, runtime),
        ("wallet", rest) => wallet(rest, runtime),
        ("zap", rest) => zap(rest, runtime),
        ("dm", rest) => dm(rest, runtime),
        ("group", rest) => group(rest, runtime),
        ("mls", rest) => mls(rest, runtime),
        ("search", rest) => search(rest, state, runtime),
        ("outbox", rest) => outbox(rest, runtime),
        ("tab", rest) => tab(rest, state),
        _ => Err(format!("unknown command: {command}; try :help")),
    };

    match result {
        Ok(CommandResult::Status(status)) => state.status = status,
        Ok(CommandResult::Action {
            correlation_id,
            label,
        }) => state.track_action(correlation_id, &label),
        Err(error) => state.status = error,
    }
}

enum CommandResult {
    Status(String),
    Action {
        correlation_id: String,
        label: String,
    },
}

trait ZapCommandRuntime {
    fn zap_identifier(
        &self,
        recipient_identifier: &str,
        amount_msats: u64,
        target_event_id: Option<&str>,
        comment: Option<&str>,
    ) -> Result<String, String>;
}

impl ZapCommandRuntime for AppRuntime {
    fn zap_identifier(
        &self,
        recipient_identifier: &str,
        amount_msats: u64,
        target_event_id: Option<&str>,
        comment: Option<&str>,
    ) -> Result<String, String> {
        AppRuntime::zap_identifier(
            self,
            recipient_identifier,
            amount_msats,
            target_event_id,
            comment,
        )
    }
}

fn help_text() -> Result<CommandResult, String> {
    Ok(CommandResult::Status(
        "commands: account profile relay dm-relays wallet zap dm group mls search outbox tab"
            .to_string(),
    ))
}

fn account(rest: &str, runtime: &AppRuntime) -> Result<CommandResult, String> {
    let (verb, args) = first_word(rest);
    match verb {
        "create" => {
            let (name, relay_args) = first_word(args);
            require(name, "account create <name> [relay...]")?;
            let relays = words(relay_args);
            runtime.create_account(name, &relays, true)?;
            Ok(status(format!("create account requested for {name}")))
        }
        "import" => {
            let nsec = args.trim();
            require(nsec, "account import <nsec>")?;
            runtime.sign_in_nsec(nsec)?;
            Ok(status("nsec sign-in requested"))
        }
        "import-mls" => {
            let nsec = args.trim();
            require(nsec, "account import-mls <nsec>")?;
            runtime.sign_in_nsec_with_marmot(nsec)?;
            Ok(status("nsec sign-in + Marmot init requested"))
        }
        "bunker" => {
            let uri = args.trim();
            require(uri, "account bunker <bunker-or-nostrconnect-uri>")?;
            runtime.sign_in_bunker(uri)?;
            Ok(status("bunker sign-in requested"))
        }
        "nostrconnect" => Ok(status(runtime.nostrconnect_uri()?)),
        "cancel-bunker" => {
            runtime.cancel_bunker();
            Ok(status("bunker handshake cancel requested"))
        }
        "switch" => {
            let id = args.trim();
            require(id, "account switch <identity-id>")?;
            runtime.switch_account(id)?;
            Ok(status(format!("switch account requested for {id}")))
        }
        "remove" => {
            let id = args.trim();
            require(id, "account remove <identity-id>")?;
            runtime.remove_account(id)?;
            Ok(status(format!("remove account requested for {id}")))
        }
        _ => Err("usage: account create|import|import-mls|bunker|nostrconnect|cancel-bunker|switch|remove".to_string()),
    }
}

fn profile(rest: &str, runtime: &AppRuntime) -> Result<CommandResult, String> {
    let (verb, args) = first_word(rest);
    match verb {
        "set" => {
            let fields = fields_from(args)?;
            let cid = runtime.publish_profile_fields(Value::Object(fields))?;
            Ok(action(cid, "profile publish"))
        }
        _ => {
            Err("usage: profile set name=<name> about=<about> picture=<url> nip05=<id>".to_string())
        }
    }
}

fn relay(rest: &str, runtime: &AppRuntime) -> Result<CommandResult, String> {
    let (verb, args) = first_word(rest);
    match verb {
        "add" => {
            let parts = words(args);
            let url = parts.first().ok_or("usage: relay add <url> [role]")?;
            let role = parts.get(1).map_or("both,indexer", String::as_str);
            runtime.add_relay(url, role)?;
            Ok(status(format!("relay add requested for {url}")))
        }
        "remove" => {
            let url = args.trim();
            require(url, "relay remove <url>")?;
            runtime.remove_relay(url)?;
            Ok(status(format!("relay remove requested for {url}")))
        }
        _ => Err("usage: relay add|remove".to_string()),
    }
}

fn dm_relays(rest: &str, runtime: &AppRuntime) -> Result<CommandResult, String> {
    let relays = words(rest);
    if relays.is_empty() {
        return Err("usage: dm-relays <relay> [relay...]".to_string());
    }
    let cid = runtime.publish_dm_relay_list(relays)?;
    Ok(action(cid, "DM relay list publish"))
}

fn wallet(rest: &str, runtime: &AppRuntime) -> Result<CommandResult, String> {
    let (verb, args) = first_word(rest);
    match verb {
        "connect" => {
            let uri = args.trim();
            require(uri, "wallet connect <nostr+walletconnect-uri>")?;
            runtime.wallet_connect(uri)?;
            Ok(status("wallet connect requested"))
        }
        "disconnect" => {
            runtime.wallet_disconnect();
            Ok(status("wallet disconnect requested"))
        }
        "pay" => {
            let (bolt11, amount) = first_word(args);
            require(bolt11, "wallet pay <bolt11> [amount_msats]")?;
            runtime.wallet_pay_invoice(bolt11, nonempty(amount))?;
            Ok(status("wallet payment requested"))
        }
        _ => Err("usage: wallet connect|disconnect|pay".to_string()),
    }
}

/// `:zap <nip05-or-lightning-address> <sats> [comment]` — dispatches a
/// Rust-owned zap intent with the raw identifier and sats→msats conversion.
/// NIP-05 lookup, LNURL fetch, and NWC pay-invoice execution stay in Rust.
fn zap(rest: &str, runtime: &AppRuntime) -> Result<CommandResult, String> {
    zap_with(rest, runtime)
}

fn zap_with(rest: &str, runtime: &impl ZapCommandRuntime) -> Result<CommandResult, String> {
    let (identifier, rest) = first_word(rest);
    require(
        identifier,
        "zap <nip05-or-lightning-address> <sats> [comment]",
    )?;
    let (sats_str, comment_rest) = first_word(rest);
    require(
        sats_str,
        "zap <nip05-or-lightning-address> <sats> [comment]",
    )?;
    let sats: u64 = sats_str
        .parse()
        .map_err(|_| "sats must be a positive integer".to_string())?;
    if sats == 0 {
        return Err("zap amount must be at least 1 sat".to_string());
    }
    let comment = nonempty(comment_rest).map(str::to_string);

    let cid = runtime.zap_identifier(
        identifier,
        sats.checked_mul(1000)
            .ok_or_else(|| "zap amount is too large".to_string())?,
        None,
        comment.as_deref(),
    )?;
    Ok(action(cid, &format!("zap {sats} sat -> {identifier}")))
}

fn dm(rest: &str, runtime: &AppRuntime) -> Result<CommandResult, String> {
    let (recipient, content) = first_word(rest);
    require(recipient, "dm <recipient-pubkey> <message>")?;
    require(content, "dm <recipient-pubkey> <message>")?;
    let cid = runtime.send_dm(recipient, content)?;
    Ok(action(cid, "DM send"))
}

fn group(rest: &str, runtime: &AppRuntime) -> Result<CommandResult, String> {
    let (verb, args) = first_word(rest);
    match verb {
        "discover" => {
            let relay = args.trim();
            require(relay, "group discover <relay-url>")?;
            let cid = runtime.discover_groups(relay)?;
            Ok(action(cid, "group discover"))
        }
        "open" => {
            let (relay, id) = first_word(args);
            require(id, "group open <relay-url> <local-id>")?;
            runtime.register_group_events(relay, id)?;
            Ok(status(format!("group chat registered for {id}")))
        }
        "join" => {
            let (relay, id) = first_word(args);
            require(id, "group join <relay-url> <local-id>")?;
            let cid = runtime.join_group(relay, id)?;
            Ok(action(cid, "group join"))
        }
        "post" => {
            let (relay, rest) = first_word(args);
            let (id, content) = first_word(rest);
            require(content, "group post <relay-url> <local-id> <message>")?;
            let cid = runtime.post_group_message(relay, id, content)?;
            Ok(action(cid, "group post"))
        }
        "react" => {
            let (relay, rest) = first_word(args);
            let (id, rest) = first_word(rest);
            let (event_id, rest) = first_word(rest);
            let (author, reaction) = first_word(rest);
            require(
                event_id,
                "group react <relay-url> <local-id> <event-id> [author] [reaction]",
            )?;
            let cid = runtime.react_group_message(
                relay,
                id,
                event_id,
                nonempty(author),
                nonempty(reaction).unwrap_or("+"),
            )?;
            Ok(action(cid, "group react"))
        }
        _ => Err("usage: group discover|open|join|post|react".to_string()),
    }
}

fn mls(rest: &str, runtime: &AppRuntime) -> Result<CommandResult, String> {
    let (verb, args) = first_word(rest);
    match verb {
        "init" => {
            runtime.marmot_register_active()?;
            Ok(status("Marmot MLS registered for active account"))
        }
        "snapshot" => Ok(status(format!(
            "mls {}",
            truncate(&runtime.marmot_snapshot_text()?)
        ))),
        "dispatch" => {
            let action: Value = serde_json::from_str(args.trim())
                .map_err(|e| format!("mls dispatch JSON parse failed: {e}"))?;
            Ok(status(format!(
                "mls {}",
                truncate(&runtime.marmot_dispatch_json(action)?)
            )))
        }
        _ => Err("usage: mls init|snapshot|dispatch <json>".to_string()),
    }
}

fn search(rest: &str, state: &mut AppState, runtime: &AppRuntime) -> Result<CommandResult, String> {
    let (kind, value) = first_word(rest);
    require(value, "search profile|thread|tag <value>")?;
    match kind {
        "profile" => {
            state.open_author_feed(runtime, value)?;
            Ok(status(format!("opened profile {value}")))
        }
        "thread" => {
            state.open_thread_feed(runtime, value)?;
            Ok(status(format!("opened thread {value}")))
        }
        "tag" => {
            runtime.open_tag(value)?;
            Ok(status(format!("opened firehose tag {value}")))
        }
        _ => Err("usage: search profile|thread|tag <value>".to_string()),
    }
}

fn outbox(rest: &str, runtime: &AppRuntime) -> Result<CommandResult, String> {
    let (verb, handle) = first_word(rest);
    require(handle, "outbox retry|cancel <handle>")?;
    match verb {
        "retry" => {
            runtime.retry_publish(handle)?;
            Ok(status(format!("retry requested for {handle}")))
        }
        "cancel" => {
            runtime.cancel_publish(handle)?;
            Ok(status(format!("cancel requested for {handle}")))
        }
        _ => Err("usage: outbox retry|cancel <handle>".to_string()),
    }
}

fn tab(rest: &str, state: &mut AppState) -> Result<CommandResult, String> {
    let tab = match rest.trim() {
        "home" => crate::features::FeatureTab::Home,
        "chats" => crate::features::FeatureTab::Chats,
        "groups" => crate::features::FeatureTab::Groups,
        "wallet" => crate::features::FeatureTab::Wallet,
        "settings" => crate::features::FeatureTab::Settings,
        _ => return Err("usage: tab home|chats|groups|wallet|settings".to_string()),
    };
    state.set_tab(tab);
    Ok(status(format!("tab {}", tab.label())))
}

fn fields_from(args: &str) -> Result<Map<String, Value>, String> {
    let mut fields = Map::new();
    for part in args.split_whitespace() {
        let (key, value) = part
            .split_once('=')
            .ok_or("profile fields must be key=value pairs")?;
        if !value.is_empty() {
            fields.insert(key.to_string(), Value::String(value.to_string()));
        }
    }
    if fields.is_empty() {
        return Err("profile set requires at least one key=value field".to_string());
    }
    Ok(fields)
}

fn first_word(input: &str) -> (&str, &str) {
    let trimmed = input.trim();
    if let Some(idx) = trimmed.find(char::is_whitespace) {
        (&trimmed[..idx], trimmed[idx..].trim())
    } else {
        (trimmed, "")
    }
}

fn words(input: &str) -> Vec<String> {
    input.split_whitespace().map(str::to_string).collect()
}

fn require<'a>(value: &'a str, usage: &str) -> Result<&'a str, String> {
    if value.trim().is_empty() {
        Err(format!("usage: {usage}"))
    } else {
        Ok(value)
    }
}

fn nonempty(value: &str) -> Option<&str> {
    let value = value.trim();
    (!value.is_empty()).then_some(value)
}

fn status(value: impl Into<String>) -> CommandResult {
    CommandResult::Status(value.into())
}

fn action(correlation_id: String, label: &str) -> CommandResult {
    CommandResult::Action {
        correlation_id,
        label: label.to_string(),
    }
}

fn truncate(value: &str) -> String {
    let compact = value.replace('\n', " ");
    if compact.chars().count() <= 120 {
        compact
    } else {
        format!("{}...", compact.chars().take(117).collect::<String>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[derive(Debug, Eq, PartialEq)]
    struct ZapCall {
        recipient_identifier: String,
        amount_msats: u64,
        target_event_id: Option<String>,
        comment: Option<String>,
    }

    #[derive(Default)]
    struct RecordingZapRuntime {
        calls: RefCell<Vec<ZapCall>>,
    }

    impl ZapCommandRuntime for RecordingZapRuntime {
        fn zap_identifier(
            &self,
            recipient_identifier: &str,
            amount_msats: u64,
            target_event_id: Option<&str>,
            comment: Option<&str>,
        ) -> Result<String, String> {
            self.calls.borrow_mut().push(ZapCall {
                recipient_identifier: recipient_identifier.to_string(),
                amount_msats,
                target_event_id: target_event_id.map(str::to_string),
                comment: comment.map(str::to_string),
            });
            Ok("cid-zap".to_string())
        }
    }

    #[test]
    fn zap_identifier_command_dispatches_raw_identifier() {
        let runtime = RecordingZapRuntime::default();
        let result = zap_with("alice@example.com 21 hi", &runtime).unwrap();
        match result {
            CommandResult::Action {
                correlation_id,
                label,
            } => {
                assert_eq!(correlation_id, "cid-zap");
                assert_eq!(label, "zap 21 sat -> alice@example.com");
            }
            CommandResult::Status(status) => panic!("expected action, got status {status}"),
        }
        assert_eq!(
            runtime.calls.into_inner(),
            vec![ZapCall {
                recipient_identifier: "alice@example.com".to_string(),
                amount_msats: 21_000,
                target_event_id: None,
                comment: Some("hi".to_string()),
            }]
        );
    }

    #[test]
    fn zap_command_source_contains_no_shell_nip05_http_resolver() {
        let source = include_str!("commands.rs");
        // Keep the source gate readable without embedding the exact old
        // resolver tokens in this test's own source text.
        let old_nip05_path = [".well-known/", "nostr", ".json"].concat();
        let old_http_client = ["ureq", "::", "AgentBuilder"].concat();
        let old_names_key = ["\"", "na", "mes", "\""].concat();
        assert!(!source.contains(&old_nip05_path));
        assert!(!source.contains(&old_http_client));
        assert!(!source.contains(&old_names_key));
    }
}
