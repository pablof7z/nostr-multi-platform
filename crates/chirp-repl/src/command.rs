#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Help,
    Parity,
    Diagnostics,
    SetRelays(Vec<String>),
    SetIndexers(Vec<String>),
    LoadKey(String),
    CreateAccount(String),
    SyncFollows,
    Home,
    Notifications,
    Profile(String),
    Thread(String),
    Search(String),
    Compose(String),
    Reply(String, String),
    React(String, String),
    Follow(String),
    Unfollow(String),
    SendDm(String, String),
    MlsInit,
    MlsStatus,
    MlsCreate(String, Vec<String>),
    MlsInvite(String, String),
    MlsAccept(String),
    MlsSend(String, String),
    MlsMessages(String),
    RawReq(String),
    Quit,
    Noop,
}

pub fn parse(line: &str) -> Result<Command, String> {
    let tokens: Vec<&str> = line.split_whitespace().collect();
    if tokens.is_empty() {
        return Ok(Command::Noop);
    }
    let rest = &tokens[1..];
    match tokens[0] {
        "help" | "?" => Ok(Command::Help),
        "parity" => no_args(rest, Command::Parity, "parity"),
        "diagnostics" | "diag" => no_args(rest, Command::Diagnostics, "diagnostics"),
        "set-relays" => urls(rest).map(Command::SetRelays),
        "set-indexers" => urls(rest).map(Command::SetIndexers),
        "load-key" => one(rest, "load-key <nsec|hex>").map(Command::LoadKey),
        "create-account" => Ok(Command::CreateAccount(if rest.is_empty() {
            "chirp-repl-user".into()
        } else {
            rest.join(" ")
        })),
        "sync-follows" => no_args(rest, Command::SyncFollows, "sync-follows"),
        "home" => no_args(rest, Command::Home, "home"),
        "notifications" | "mentions" => no_args(rest, Command::Notifications, "notifications"),
        "profile" => one(rest, "profile <npub|nprofile|hex>").map(Command::Profile),
        "thread" => one(rest, "thread <note|nevent|hex>").map(Command::Thread),
        "search" => one(rest, "search #tag").map(Command::Search),
        "compose" => text(rest, "compose <text>").map(Command::Compose),
        "reply" => {
            if rest.len() < 2 {
                return Err("reply <note|nevent|hex> <text>".into());
            }
            Ok(Command::Reply(rest[0].into(), rest[1..].join(" ")))
        }
        "react" => {
            if rest.is_empty() || rest.len() > 2 {
                return Err("react <note|nevent|hex> [reaction]".into());
            }
            Ok(Command::React(
                rest[0].into(),
                rest.get(1).copied().unwrap_or("+").into(),
            ))
        }
        "follow" => one(rest, "follow <npub|nprofile|hex>").map(Command::Follow),
        "unfollow" => one(rest, "unfollow <npub|nprofile|hex>").map(Command::Unfollow),
        "send-dm" | "dm" => {
            if rest.len() < 2 {
                return Err("send-dm <npub|nprofile|hex> <text>".into());
            }
            Ok(Command::SendDm(rest[0].into(), rest[1..].join(" ")))
        }
        "mls-init" => no_args(rest, Command::MlsInit, "mls-init"),
        "mls-status" => no_args(rest, Command::MlsStatus, "mls-status"),
        "mls-create" => {
            if rest.is_empty() {
                return Err("mls-create <name> [npub|hex ...]".into());
            }
            Ok(Command::MlsCreate(
                rest[0].to_string(),
                rest[1..].iter().map(std::string::ToString::to_string).collect(),
            ))
        }
        "mls-invite" => {
            if rest.len() != 2 {
                return Err("mls-invite <group_id_hex> <npub|hex>".into());
            }
            Ok(Command::MlsInvite(rest[0].into(), rest[1].into()))
        }
        "mls-accept" => one(rest, "mls-accept <welcome_id_hex|first>").map(Command::MlsAccept),
        "mls-send" => {
            if rest.len() < 2 {
                return Err("mls-send <group_id_hex> <text>".into());
            }
            Ok(Command::MlsSend(rest[0].into(), rest[1..].join(" ")))
        }
        "mls-messages" => one(rest, "mls-messages <group_id_hex>").map(Command::MlsMessages),
        "raw-req" => text(rest, "raw-req <json-filter>").map(Command::RawReq),
        "quit" | "exit" => Ok(Command::Quit),
        other => Err(format!("unknown command '{other}' (try help)")),
    }
}

fn no_args(args: &[&str], command: Command, usage: &str) -> Result<Command, String> {
    if args.is_empty() {
        Ok(command)
    } else {
        Err(format!("{usage} takes no arguments"))
    }
}

fn one(args: &[&str], usage: &str) -> Result<String, String> {
    if args.len() == 1 {
        Ok(args[0].into())
    } else {
        Err(usage.into())
    }
}

fn text(args: &[&str], usage: &str) -> Result<String, String> {
    if args.is_empty() {
        Err(usage.into())
    } else {
        Ok(args.join(" "))
    }
}

fn urls(args: &[&str]) -> Result<Vec<String>, String> {
    if args.is_empty() {
        return Err("expected one or more ws:// or wss:// relay URLs".into());
    }
    let mut out = Vec::new();
    for arg in args {
        for url in arg.split(',').filter(|s| !s.is_empty()) {
            if !(url.starts_with("wss://") || url.starts_with("ws://")) {
                return Err(format!("invalid relay URL '{url}'"));
            }
            out.push(url.trim_end_matches('/').to_string());
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_read_commands() {
        assert_eq!(parse("home").unwrap(), Command::Home);
        assert_eq!(parse("notifications").unwrap(), Command::Notifications);
        assert_eq!(
            parse("profile npub1abc").unwrap(),
            Command::Profile("npub1abc".into())
        );
        assert_eq!(
            parse("search #nostr").unwrap(),
            Command::Search("#nostr".into())
        );
    }

    #[test]
    fn parses_write_commands() {
        assert_eq!(
            parse("compose hello world").unwrap(),
            Command::Compose("hello world".into())
        );
        assert_eq!(
            parse("reply note1abc hi there").unwrap(),
            Command::Reply("note1abc".into(), "hi there".into())
        );
        assert_eq!(
            parse("react note1abc").unwrap(),
            Command::React("note1abc".into(), "+".into())
        );
        assert_eq!(
            parse("send-dm npub1abc hello there").unwrap(),
            Command::SendDm("npub1abc".into(), "hello there".into())
        );
        assert_eq!(
            parse("dm npub1abc hi").unwrap(),
            Command::SendDm("npub1abc".into(), "hi".into())
        );
    }

    #[test]
    fn parses_relay_and_diagnostic_commands() {
        assert_eq!(
            parse("set-relays wss://relay.primal.net,wss://purplepag.es").unwrap(),
            Command::SetRelays(vec![
                "wss://relay.primal.net".into(),
                "wss://purplepag.es".into()
            ])
        );
        assert_eq!(
            parse("set-indexers wss://purplepag.es").unwrap(),
            Command::SetIndexers(vec!["wss://purplepag.es".into()])
        );
        assert_eq!(parse("diag").unwrap(), Command::Diagnostics);
        assert_eq!(parse("parity").unwrap(), Command::Parity);
        assert_eq!(
            parse("raw-req {\"kinds\":[1]}").unwrap(),
            Command::RawReq("{\"kinds\":[1]}".into())
        );
        assert_eq!(
            parse("mls-create test abc def").unwrap(),
            Command::MlsCreate("test".into(), vec!["abc".into(), "def".into()])
        );
        assert_eq!(
            parse("mls-send abc hello mls").unwrap(),
            Command::MlsSend("abc".into(), "hello mls".into())
        );
    }

    #[test]
    fn rejects_bad_shapes() {
        assert!(parse("profile").is_err());
        assert!(parse("reply note1abc").is_err());
        assert!(parse("react").is_err());
        assert!(parse("send-dm").is_err());
        assert!(parse("send-dm npub1abc").is_err());
        assert!(parse("set-relays https://bad.example").is_err());
        assert!(parse("unknown").is_err());
    }
}
