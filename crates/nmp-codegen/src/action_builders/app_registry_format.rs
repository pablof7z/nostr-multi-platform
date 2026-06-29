//! JSON shape for app-local action-builder registry input.

use std::path::PathBuf;

use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RegistryDocument {
    pub actions: Vec<ActionContractRow>,
    pub outputs: OutputsRow,
    pub drift_checks: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ActionContractRow {
    pub action_namespace: String,
    pub event_kind: u32,
    pub dispatch: DispatchKind,
    pub schema: SchemaRow,
    pub builder: BuilderRow,
    pub rust: RustOwnerRow,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum DispatchKind {
    PublishesEvent,
    AppLocal,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SchemaRow {
    pub schema_path: PathBuf,
    pub root_type: String,
    pub file_identifier: String,
    pub schema_id: String,
    pub schema_version: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct BuilderRow {
    pub method: String,
    pub doc: String,
    pub fields: Vec<FieldRow>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct FieldRow {
    pub name: String,
    pub kind: ContractFieldKind,
    #[serde(default)]
    pub optional: bool,
    pub presence_flag: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ContractFieldKind {
    String,
    Uint,
    StringVec,
    UintVec,
    Ulong,
    UlongWithPresenceFlag,
    RelayListEntryVec,
    Ubyte,
    Sbyte,
    GroupRef,
    StringTagVec,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RustOwnerRow {
    pub rust_crate: String,
    pub module: String,
    pub payload_type: String,
    pub action_module: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct OutputsRow {
    pub swift: PathBuf,
    pub kotlin: PathBuf,
    pub ts: PathBuf,
}
