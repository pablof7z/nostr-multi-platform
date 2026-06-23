//! WRITE-direction typed FlatBuffers action payload codecs (ADR-0064 / S9
//! #1747).
//!
//! Each event-authoring NIP-29 `ActionModule` carries its `start()` input as the
//! OPAQUE `DispatchEnvelope.payload`. The registry adapter decodes those bytes
//! through [`nmp_core::substrate::ActionPayload::decode`] — the single
//! typed-decode site — running the fail-closed `schema_version` gate BEFORE
//! `start()`. A version trip or any structural error is reported as a
//! data-shaped [`nmp_core::substrate::ActionPayloadDecodeError`]; there are NO
//! panics on the decode path (D6).
//!
//! The impls are split by action family to stay under the file-size cap:
//! - [`group`] — `join` / `leave` / `post_chat_message` / `create_public_group`
//!   / `react_in_group`.
//! - [`group_event`] — `share_event_in_group` / `repost_in_group`.
//! - [`admin`] — `put_user` / `create_invite`.
//! - [`discover`] — `discover_groups` (`nmp.nip29.discover`).
//!
//! Each generated module below is intrinsically `unsafe` (every accessor reads a
//! raw `Table`); only the generated modules opt back into `unsafe`. The
//! hand-written codecs use none.

macro_rules! generated_action_module {
    ($module:ident, $file:literal) => {
        #[allow(
            clippy::all,
            dead_code,
            deprecated,
            missing_docs,
            non_camel_case_types,
            non_snake_case,
            unsafe_code,
            unused_imports
        )]
        #[path = $file]
        pub mod $module;
    };
}

generated_action_module!(
    join_group_action_generated,
    "../generated/join_group_action_generated.rs"
);
generated_action_module!(
    leave_group_action_generated,
    "../generated/leave_group_action_generated.rs"
);
generated_action_module!(
    post_chat_message_action_generated,
    "../generated/post_chat_message_action_generated.rs"
);
generated_action_module!(
    react_in_group_action_generated,
    "../generated/react_in_group_action_generated.rs"
);
generated_action_module!(
    create_public_group_action_generated,
    "../generated/create_public_group_action_generated.rs"
);
generated_action_module!(
    share_event_in_group_action_generated,
    "../generated/share_event_in_group_action_generated.rs"
);
generated_action_module!(
    repost_in_group_action_generated,
    "../generated/repost_in_group_action_generated.rs"
);
generated_action_module!(
    put_user_action_generated,
    "../generated/put_user_action_generated.rs"
);
generated_action_module!(
    create_invite_action_generated,
    "../generated/create_invite_action_generated.rs"
);
generated_action_module!(
    discover_groups_action_generated,
    "../generated/discover_groups_action_generated.rs"
);
generated_action_module!(
    set_parent_action_generated,
    "../generated/set_parent_action_generated.rs"
);

pub mod admin;
pub mod discover;
pub mod group;
pub mod group_event;
pub mod subgroups;

use nmp_core::substrate::ActionPayloadDecodeError;

/// Wire schema version for every nip29 action payload. Bump on any breaking
/// change to a `schema/*_action.fbs` table.
pub const SCHEMA_VERSION: u32 = 1;

/// Construct a data-shaped [`ActionPayloadDecodeError::Malformed`] (D6).
fn malformed(reason: impl Into<String>) -> ActionPayloadDecodeError {
    ActionPayloadDecodeError::Malformed {
        reason: reason.into(),
    }
}

/// Read-and-gate the raw `schema_version` field BEFORE any further field reads.
/// Returns the version trip as a fail-closed error; never panics.
fn gate_schema_version(found: u32) -> Result<(), ActionPayloadDecodeError> {
    if found != SCHEMA_VERSION {
        return Err(ActionPayloadDecodeError::SchemaVersionMismatch {
            found,
            expected: SCHEMA_VERSION,
        });
    }
    Ok(())
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;

#[cfg(test)]
#[path = "tests_fail_closed.rs"]
mod tests_fail_closed;
