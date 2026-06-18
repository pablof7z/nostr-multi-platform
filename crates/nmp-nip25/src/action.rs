use nmp_core::substrate::{
    ActionContext, ActionModule, ActionRegistrar, ActionRejection, ProtocolCommand,
    ProtocolCommandContext, ProtocolCommandError, UnsignedEvent,
};
use nmp_core::ActorCommand;
use serde::{Deserialize, Serialize};

pub const KIND_REACTION: u32 = 7;
pub const KIND_REACTION_DELETE: u32 = 5;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReactAction {
    pub target_event_id: String,
    #[serde(default = "default_reaction")]
    pub reaction: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_author_pubkey: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct UnreactAction {
    pub reaction_event_id: String,
    #[serde(default)]
    pub reason: String,
}

#[derive(Debug)]
pub struct PublishReactionCommand {
    action: ReactAction,
    correlation_id: String,
}

#[derive(Debug)]
pub struct UnreactReactionCommand {
    action: UnreactAction,
    correlation_id: String,
}

pub struct ReactModule;
pub struct UnreactModule;

impl ActionModule for ReactModule {
    const NAMESPACE: &'static str = "nmp.nip25.react";
    type Action = ReactAction;

    fn start(&self, _ctx: &mut ActionContext, action: Self::Action) -> Result<(), ActionRejection> {
        validate_react(&action)
    }

    fn execute(
        &self,
        action: Self::Action,
        correlation_id: &str,
        send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        send(ActorCommand::Protocol(Box::new(PublishReactionCommand {
            action,
            correlation_id: correlation_id.to_string(),
        })));
        Ok(())
    }
}

impl ActionModule for UnreactModule {
    const NAMESPACE: &'static str = "nmp.nip25.unreact";
    type Action = UnreactAction;

    fn start(&self, _ctx: &mut ActionContext, action: Self::Action) -> Result<(), ActionRejection> {
        if !is_hex64(&action.reaction_event_id) {
            return Err(ActionRejection::Invalid(
                "unreact requires a 64-hex reaction_event_id".to_string(),
            ));
        }
        Ok(())
    }

    fn execute(
        &self,
        action: Self::Action,
        correlation_id: &str,
        send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        send(ActorCommand::Protocol(Box::new(UnreactReactionCommand {
            action,
            correlation_id: correlation_id.to_string(),
        })));
        Ok(())
    }
}

impl ProtocolCommand for PublishReactionCommand {
    fn run(
        self: Box<Self>,
        ctx: &mut ProtocolCommandContext<'_>,
    ) -> Result<(), ProtocolCommandError> {
        let Some((tags, content)) = reaction_tags(&self.action) else {
            return Err(ProtocolCommandError::new(
                "react: malformed target event id",
            ));
        };
        ctx.send(ActorCommand::PublishUnsignedEvent {
            event: UnsignedEvent {
                pubkey: String::new(),
                kind: KIND_REACTION,
                tags,
                content,
                created_at: 0,
            },
            correlation_id: Some(self.correlation_id),
            signer_pubkey: None,
        });
        Ok(())
    }
}

impl ProtocolCommand for UnreactReactionCommand {
    fn run(
        self: Box<Self>,
        ctx: &mut ProtocolCommandContext<'_>,
    ) -> Result<(), ProtocolCommandError> {
        ctx.send(ActorCommand::PublishUnsignedEvent {
            event: UnsignedEvent {
                pubkey: String::new(),
                kind: KIND_REACTION_DELETE,
                tags: vec![vec!["e".to_string(), self.action.reaction_event_id]],
                content: self.action.reason,
                created_at: 0,
            },
            correlation_id: Some(self.correlation_id),
            signer_pubkey: None,
        });
        Ok(())
    }
}

pub fn register_actions(app: &mut impl ActionRegistrar) {
    app.register_default_action(ReactModule);
    app.register_default_action(UnreactModule);
}

fn validate_react(action: &ReactAction) -> Result<(), ActionRejection> {
    if !is_hex64(&action.target_event_id) {
        return Err(ActionRejection::Invalid(
            "react requires a 64-hex target_event_id".to_string(),
        ));
    }
    if action
        .target_author_pubkey
        .as_deref()
        .is_some_and(|author| !is_hex64(author))
    {
        return Err(ActionRejection::Invalid(
            "react target_author_pubkey must be 64-hex when provided".to_string(),
        ));
    }
    Ok(())
}

fn reaction_tags(action: &ReactAction) -> Option<(Vec<Vec<String>>, String)> {
    if !is_hex64(&action.target_event_id) {
        return None;
    }
    let content = if action.reaction.trim().is_empty() {
        "+".to_string()
    } else {
        action.reaction.clone()
    };
    let mut tags = vec![vec!["e".to_string(), action.target_event_id.clone()]];
    if let Some(author) = &action.target_author_pubkey {
        tags.push(vec!["p".to_string(), author.clone()]);
    }
    Some((tags, content))
}

fn default_reaction() -> String {
    "+".to_string()
}

fn is_hex64(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

#[cfg(test)]
#[path = "action_tests.rs"]
mod tests;
