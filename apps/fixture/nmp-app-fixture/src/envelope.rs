use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "t", content = "v", rename_all = "snake_case")]
pub enum UpdateEnvelope {
    /// A discrete update — apply as a delta. Carries `schema_version`.
    Update(nmp_core::DeltaEnvelope),
    /// A full snapshot — replace rendered state.
    Snapshot(serde_json::Value),
    /// Actor-thread death (D7) — terminal; surface a fatal error.
    Panic(nmp_core::PanicFrame),
}
