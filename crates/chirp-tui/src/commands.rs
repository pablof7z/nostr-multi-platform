use serde_json::{Map, Value};

use crate::app::{AppRuntime, AppState, Pane};

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

fn help_text() -> Result<CommandResult, String> {
    Ok(CommandResult::Status(
        "commands: account profile relay dm-relays wallet dm group mls search outbox tab"
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
            let role = parts.get(1).map(String::as_str).unwrap_or("both,indexer");
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
            runtime.register_group_chat(relay, id)?;
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
        "reply" => {
            let (relay, rest) = first_word(args);
            let (id, rest) = first_word(rest);
            let (parent, content) = first_word(rest);
            require(
                content,
                "group reply <relay-url> <local-id> <event-id> <message>",
            )?;
            let cid = runtime.reply_group_message(relay, id, parent, content)?;
            Ok(action(cid, "group reply"))
        }
        _ => Err("usage: group discover|open|join|post|react|reply".to_string()),
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
            runtime.open_author(value)?;
            state.focus(Pane::Profile);
            Ok(status(format!("opened profile {value}")))
        }
        "thread" => {
            runtime.open_thread(value)?;
            state.focus(Pane::Detail);
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
