use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum KernelAction {
    Start,
    Stop,
    OpenView { namespace: String, key: String },
    CloseView { namespace: String, key: String },
    RunDiagnostics,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum KernelUpdate {
    Started { rev: u64 },
    Stopped { rev: u64 },
    ViewOpened { namespace: String, key: String },
    ViewClosed { namespace: String, key: String },
    Diagnostics { summary: String },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum KernelViewSpec {
    Diagnostics,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct AppState {
    pub rev: u64,
    pub open_view_count: usize,
}
