//! Emitter input wrapper for built-in and app-local action-builder registries.

use super::registry::{ActionBuilder, ACTION_BUILDERS};

/// Wire identity needed by the flat-table action-builder emitters.
#[derive(Clone, Copy)]
pub struct ActionBuilderWireContract {
    /// Root payload table schema version, written at slot 0.
    pub schema_version: u32,
    /// FlatBuffers file identifier passed to `finish`.
    pub file_identifier: &'static str,
}

/// App-local wire identity row, keyed by `DispatchEnvelope.action_namespace`.
pub struct AppActionBuilderWireContract {
    /// Action namespace stamped into the generated dispatch envelope.
    pub namespace: &'static str,
    /// Payload table wire identity for this namespace.
    pub contract: ActionBuilderWireContract,
}

/// Complete input to the generated flat-table action-builder emitters.
pub struct ActionBuilderRegistry<'a> {
    /// Flat-table builder methods to render, in declaration order.
    pub builders: &'a [ActionBuilder],
    /// App-local namespace to payload wire contract rows.
    pub app_contracts: &'a [AppActionBuilderWireContract],
    /// Whether to append NMP's built-in union builders after flat-table methods.
    pub include_builtin_unions: bool,
}

impl<'a> ActionBuilderRegistry<'a> {
    /// Built-in NMP action builders plus built-in union helpers.
    #[must_use]
    pub fn builtin() -> Self {
        Self {
            builders: ACTION_BUILDERS,
            app_contracts: &[],
            include_builtin_unions: true,
        }
    }

    /// Built-in flat-table subset. Used by tests that want to render a slice.
    #[must_use]
    pub fn builtin_slice(builders: &'a [ActionBuilder]) -> Self {
        Self {
            builders,
            app_contracts: &[],
            include_builtin_unions: std::ptr::eq(builders.as_ptr(), ACTION_BUILDERS.as_ptr()),
        }
    }

    /// App-local flat-table builder registry. Does not append NMP union helpers.
    #[must_use]
    pub fn app_local(
        builders: &'a [ActionBuilder],
        app_contracts: &'a [AppActionBuilderWireContract],
    ) -> Self {
        Self {
            builders,
            app_contracts,
            include_builtin_unions: false,
        }
    }

    /// Resolve payload wire facts for a builder namespace.
    #[must_use]
    pub fn wire_contract_for(&self, namespace: &str) -> ActionBuilderWireContract {
        if let Some(row) = self
            .app_contracts
            .iter()
            .find(|row| row.namespace == namespace)
        {
            return row.contract;
        }
        let contract = crate::action_contract::contract_for(namespace);
        ActionBuilderWireContract {
            schema_version: contract.schema_version,
            file_identifier: contract.file_identifier,
        }
    }

    /// True when this registry is the full built-in NMP registry.
    #[must_use]
    pub fn is_full_builtin(&self) -> bool {
        self.include_builtin_unions
            && std::ptr::eq(self.builders.as_ptr(), ACTION_BUILDERS.as_ptr())
    }
}
