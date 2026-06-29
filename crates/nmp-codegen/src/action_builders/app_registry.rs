//! App-local static action-builder registry input (#2411).
//!
//! This parser intentionally lives in `nmp-codegen`: app-private contracts are
//! codegen input, not rows in NMP's default action contract table and not
//! runtime-loaded plugin metadata.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use super::app_registry_format::{
    ActionContractRow, ContractFieldKind, DispatchKind, FieldRow, RegistryDocument,
};
use super::registry::{ActionBuilder, FieldKind, PayloadField};
use super::Platform;
use super::{ActionBuilderRegistry, ActionBuilderWireContract, AppActionBuilderWireContract};

/// Parsed app-local action-builder registry.
pub struct LoadedAppActionBuilderRegistry {
    /// Flat-table action builders, in app contract declaration order.
    pub builders: Vec<ActionBuilder>,
    /// Namespace-keyed payload wire contracts used by the emitters.
    pub wire_contracts: Vec<AppActionBuilderWireContract>,
    /// App-declared generated-builder output paths.
    pub outputs: AppActionBuilderOutputs,
}

impl LoadedAppActionBuilderRegistry {
    /// Borrow this loaded app registry as an emitter input.
    #[must_use]
    pub fn as_registry(&self) -> ActionBuilderRegistry<'_> {
        ActionBuilderRegistry::app_local(&self.builders, &self.wire_contracts)
    }

    /// Return the app-declared output path for a platform.
    #[must_use]
    pub fn output_for(&self, platform: Platform) -> &Path {
        match platform {
            Platform::Swift => &self.outputs.swift,
            Platform::Kotlin => &self.outputs.kotlin,
            Platform::Ts => &self.outputs.ts,
        }
    }
}

/// App-declared generated-builder output paths.
pub struct AppActionBuilderOutputs {
    /// Swift generated-builder output path.
    pub swift: PathBuf,
    /// Kotlin generated-builder output path.
    pub kotlin: PathBuf,
    /// TypeScript generated-builder output path.
    pub ts: PathBuf,
}

/// Load and parse an app-local action-builder registry JSON file.
///
/// # Errors
/// Filesystem failures, invalid JSON, or invalid contract rows.
pub fn load_app_action_builder_registry(
    path: &Path,
) -> Result<LoadedAppActionBuilderRegistry, String> {
    let raw = std::fs::read_to_string(path)
        .map_err(|e| format!("read app action-builder registry {}: {e}", path.display()))?;
    parse_app_action_builder_registry(&raw)
}

/// Parse an app-local action-builder registry JSON document.
///
/// # Errors
/// Invalid JSON or invalid contract rows.
pub fn parse_app_action_builder_registry(
    raw: &str,
) -> Result<LoadedAppActionBuilderRegistry, String> {
    let doc: RegistryDocument = serde_json::from_str(raw)
        .map_err(|e| format!("parse app action-builder registry JSON: {e}"))?;
    if doc.actions.is_empty() {
        return Err("app action-builder registry must declare at least one action".to_string());
    }
    if doc.drift_checks.is_empty() {
        return Err("app action-builder registry must declare drift_checks".to_string());
    }

    let mut builders = Vec::with_capacity(doc.actions.len());
    let mut contracts_by_namespace: BTreeMap<String, ActionBuilderWireContract> = BTreeMap::new();
    let mut methods = BTreeSet::new();

    for action in doc.actions {
        validate_action(&action)?;
        if !methods.insert(action.builder.method.clone()) {
            return Err(format!(
                "duplicate generated builder method {:?}",
                action.builder.method
            ));
        }

        let wire = ActionBuilderWireContract {
            schema_version: action.schema.schema_version,
            file_identifier: leak_str(action.schema.file_identifier.clone()),
        };
        match contracts_by_namespace.get(&action.action_namespace) {
            Some(existing)
                if existing.schema_version != wire.schema_version
                    || existing.file_identifier != wire.file_identifier =>
            {
                return Err(format!(
                    "namespace {:?} has conflicting schema_version/file_identifier rows",
                    action.action_namespace
                ));
            }
            Some(_) => {}
            None => {
                contracts_by_namespace.insert(action.action_namespace.clone(), wire);
            }
        }

        let fields = action
            .builder
            .fields
            .into_iter()
            .map(payload_field)
            .collect::<Result<Vec<_>, _>>()?;
        let fields = Box::leak(fields.into_boxed_slice());
        builders.push(ActionBuilder {
            namespace: leak_str(action.action_namespace),
            method: leak_str(action.builder.method),
            fields,
            doc: leak_str(action.builder.doc),
        });
    }

    let wire_contracts = contracts_by_namespace
        .into_iter()
        .map(|(namespace, contract)| AppActionBuilderWireContract {
            namespace: leak_str(namespace),
            contract,
        })
        .collect();

    Ok(LoadedAppActionBuilderRegistry {
        builders,
        wire_contracts,
        outputs: AppActionBuilderOutputs {
            swift: doc.outputs.swift,
            kotlin: doc.outputs.kotlin,
            ts: doc.outputs.ts,
        },
    })
}

fn validate_action(action: &ActionContractRow) -> Result<(), String> {
    if action.action_namespace.trim().is_empty() {
        return Err("action_namespace must not be empty".to_string());
    }
    if action.schema.schema_id.trim().is_empty() {
        return Err(format!(
            "action {:?} schema_id must not be empty",
            action.action_namespace
        ));
    }
    if action.schema.schema_path.as_os_str().is_empty() {
        return Err(format!(
            "action {:?} schema_path must not be empty",
            action.action_namespace
        ));
    }
    if action.schema.root_type.trim().is_empty() {
        return Err(format!(
            "action {:?} root_type must not be empty",
            action.action_namespace
        ));
    }
    if action.schema.file_identifier.chars().count() != 4 {
        return Err(format!(
            "action {:?} file_identifier must be exactly four characters",
            action.action_namespace
        ));
    }
    if action.builder.method.trim().is_empty() {
        return Err(format!(
            "action {:?} builder.method must not be empty",
            action.action_namespace
        ));
    }
    if action.builder.doc.trim().is_empty() {
        return Err(format!(
            "action {:?} builder.doc must not be empty",
            action.action_namespace
        ));
    }
    if action.rust.rust_crate.trim().is_empty()
        || action.rust.module.trim().is_empty()
        || action.rust.payload_type.trim().is_empty()
        || action.rust.action_module.trim().is_empty()
    {
        return Err(format!(
            "action {:?} rust owner fields must not be empty",
            action.action_namespace
        ));
    }
    let _ = action.event_kind;
    match action.dispatch {
        DispatchKind::PublishesEvent | DispatchKind::AppLocal => {}
    }
    Ok(())
}

fn payload_field(row: FieldRow) -> Result<PayloadField, String> {
    if row.name.trim().is_empty() {
        return Err("builder field name must not be empty".to_string());
    }
    let kind = match row.kind {
        ContractFieldKind::String => FieldKind::Str,
        ContractFieldKind::Uint => FieldKind::Uint,
        ContractFieldKind::StringVec => FieldKind::StrVec,
        ContractFieldKind::UintVec => FieldKind::UintVec,
        ContractFieldKind::Ulong => FieldKind::Ulong,
        ContractFieldKind::UlongWithPresenceFlag => FieldKind::UlongWithPresenceFlag {
            flag_name: leak_str(row.presence_flag.ok_or_else(|| {
                format!(
                    "field {:?} kind ulong_with_presence_flag requires presence_flag",
                    row.name
                )
            })?),
        },
        ContractFieldKind::RelayListEntryVec => FieldKind::RelayListEntryVec,
        ContractFieldKind::Ubyte => FieldKind::Ubyte,
        ContractFieldKind::Sbyte => FieldKind::Sbyte,
        ContractFieldKind::GroupRef => FieldKind::GroupRef,
        ContractFieldKind::StringTagVec => FieldKind::StringTagVec,
    };
    Ok(PayloadField {
        name: leak_str(row.name),
        kind,
        optional: row.optional,
    })
}

fn leak_str(value: String) -> &'static str {
    Box::leak(value.into_boxed_str())
}

#[cfg(test)]
#[path = "app_registry_tests.rs"]
mod tests;
