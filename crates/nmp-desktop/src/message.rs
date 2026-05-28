use std::sync::mpsc::Sender;

use iced::widget::text_editor;
use nmp_core::testing::ActorCommand;

use crate::snapshot::Snapshot;

/// Messages that drive the iced application.
#[derive(Debug, Clone)]
pub enum Message {
    /// The bridge subscription has connected to the kernel and is handing
    /// over the command sender so the UI can dispatch actions.
    BridgeReady(Sender<ActorCommand>),
    /// A new kernel snapshot has arrived.
    SnapshotUpdated(Snapshot),
    /// Compose text editor action.
    ComposeAction(text_editor::Action),
    /// Nsec text buffer changed.
    NsecChanged(String),
    /// User clicked "Publish".
    Publish,
    /// User clicked "Create new account".
    CreateAccount,
    /// User clicked "Sign in".
    SignIn,
    /// Timeline (re)opened after account creation / sign-in.
    OpenTimeline,
    /// A URL inside a note body was tapped.
    OpenUrl(String),
}
