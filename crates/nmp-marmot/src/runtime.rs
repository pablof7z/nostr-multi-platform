//! Marmot explicit-composition installer and active-identity runtime.
//!
//! This module is the normal crate-owned install seam for Marmot. It registers
//! the action module, ingest parser, identity observer, and typed projection
//! without reviving any Marmot-specific C ABI or app-owned lifecycle.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use nmp_core::actor::{ActorCommand, InterestsCommand};
use nmp_core::slots::MlsLocalNsecSlot;
use nmp_core::substrate::{
    ActionRegistrar, HostCapabilities, IdentityChangeRegistrar, IngestParser,
    IngestParserRegistrar, RegistrationError, SnapshotProjectionRegistrar,
};
use nostr::nips::nip19::FromBech32;
use nostr::{Keys, SecretKey};

use crate::interest::{
    giftwrap_inbox_identity, giftwrap_inbox_interest, KIND_GIFT_WRAP, KIND_MARMOT_GROUP_MESSAGE,
    KIND_MARMOT_KEY_PACKAGE, KIND_MARMOT_WELCOME,
};
use crate::projection::action::MarmotActionModule;
use crate::projection::payload::MarmotSnapshot;
use crate::projection::state::MarmotProjection;
use crate::projection::tap::MarmotIngestParser;
use crate::service::MarmotService;
use crate::wire::{messages_fb, snapshot_fb};

/// Slot key used for every Marmot ingest parser registration.
pub const MARMOT_INGEST_SLOT: &str = "nmp.marmot";

const MARMOT_INGEST_KINDS: [u32; 4] = [
    KIND_GIFT_WRAP,
    KIND_MARMOT_GROUP_MESSAGE,
    KIND_MARMOT_KEY_PACKAGE,
    KIND_MARMOT_WELCOME,
];
const MARMOT_MESSAGE_PAGE: usize = 200;

/// Marmot-owned credential slot wrapper.
///
/// Hosts hand over the actor-owned MLS nsec slot; only `nmp-marmot` reads and
/// parses the raw key material.
#[derive(Clone)]
pub struct MarmotLocalCredentialSlot {
    slot: MlsLocalNsecSlot,
}

impl MarmotLocalCredentialSlot {
    #[must_use]
    pub fn new(slot: MlsLocalNsecSlot) -> Self {
        Self { slot }
    }

    fn active_keys(&self) -> Option<Keys> {
        let nsec = self.slot.lock().ok()?.clone()?;
        let secret = SecretKey::from_bech32(nsec.as_str()).ok()?;
        Some(Keys::new(secret))
    }
}

/// Configuration for [`install`].
#[derive(Clone)]
pub struct MarmotConfig {
    storage_dir: PathBuf,
    credential_slot: MarmotLocalCredentialSlot,
    service_id: String,
    db_key_prefix: String,
}

impl MarmotConfig {
    /// Build a Marmot config from an app-support directory and the Marmot-owned
    /// local credential slot wrapper.
    #[must_use]
    pub fn new(
        storage_dir: impl Into<PathBuf>,
        credential_slot: MarmotLocalCredentialSlot,
    ) -> Self {
        Self {
            storage_dir: storage_dir.into(),
            credential_slot,
            service_id: "nmp-marmot".to_string(),
            db_key_prefix: "marmot-mls-state".to_string(),
        }
    }

    /// Override keyring coordinates for hosts that need app-specific service
    /// ids while keeping the raw-key read sealed in this crate.
    #[must_use]
    pub fn with_keyring_ids(
        mut self,
        service_id: impl Into<String>,
        db_key_prefix: impl Into<String>,
    ) -> Self {
        self.service_id = service_id.into();
        self.db_key_prefix = db_key_prefix.into();
        self
    }

    fn db_path_for(&self, pubkey_hex: &str) -> PathBuf {
        self.storage_dir
            .join(format!("marmot-mls-state-{pubkey_hex}.sqlite"))
    }

    fn db_key_id_for(&self, pubkey_hex: &str) -> String {
        format!("{}.{}", self.db_key_prefix, pubkey_hex)
    }
}

#[derive(Debug)]
pub enum MarmotInstallError {
    ActionRegistration(RegistrationError),
}

impl From<RegistrationError> for MarmotInstallError {
    fn from(value: RegistrationError) -> Self {
        Self::ActionRegistration(value)
    }
}

/// Stable runtime followed by the action module, ingest parser, and projection
/// closure across account switches.
pub(crate) struct MarmotRuntime {
    config: MarmotConfig,
    actor_sender: nmp_core::CommandSender,
    active: Mutex<Option<ActiveMarmotProjection>>,
}

struct ActiveMarmotProjection {
    pubkey_hex: String,
    projection: Arc<MarmotProjection>,
}

impl MarmotRuntime {
    fn new(config: MarmotConfig, actor_sender: nmp_core::CommandSender) -> Self {
        Self {
            config,
            actor_sender,
            active: Mutex::new(None),
        }
    }

    #[cfg(test)]
    pub(crate) fn from_projection_for_tests(projection: Arc<MarmotProjection>) -> Arc<Self> {
        Arc::new(Self {
            config: MarmotConfig::new(
                PathBuf::from("."),
                MarmotLocalCredentialSlot::new(Arc::new(Mutex::new(None))),
            ),
            actor_sender: nmp_core::CommandSender::bounded_channel().0,
            active: Mutex::new(Some(ActiveMarmotProjection {
                pubkey_hex: "test".to_string(),
                projection,
            })),
        })
    }

    /// Rebind to the active identity. `None`, remote-signer accounts, invalid
    /// local keys, and service-init failure all clear the active projection.
    pub(crate) fn rebind_for_identity(&self, active_pubkey: Option<String>) {
        let Some(pubkey_hex) = active_pubkey else {
            self.clear_active();
            return;
        };

        if self
            .active
            .lock()
            .ok()
            .and_then(|active| active.as_ref().map(|p| p.pubkey_hex == pubkey_hex))
            .unwrap_or(false)
        {
            return;
        }

        let Some(keys) = self.config.credential_slot.active_keys() else {
            self.clear_active();
            return;
        };
        if keys.public_key().to_hex() != pubkey_hex {
            self.clear_active();
            return;
        }

        let db_path = self.config.db_path_for(&pubkey_hex);
        let db_key_id = self.config.db_key_id_for(&pubkey_hex);
        let Ok(service) = MarmotService::new(db_path, &self.config.service_id, &db_key_id, keys)
        else {
            self.clear_active();
            return;
        };

        let projection = Arc::new(MarmotProjection::new(service, None));
        projection.set_actor_sender(self.actor_sender.clone());
        let previous = self.replace_active(pubkey_hex.clone(), Arc::clone(&projection));
        if let Some(previous) = previous {
            self.drop_projection_interests(previous);
        }
        self.subscribe_giftwrap_inbox(&pubkey_hex);
        projection.resubscribe_all_groups();
    }

    #[must_use]
    pub(crate) fn projection(&self) -> Option<Arc<MarmotProjection>> {
        self.active
            .lock()
            .ok()
            .and_then(|active| active.as_ref().map(|p| Arc::clone(&p.projection)))
    }

    #[must_use]
    pub(crate) fn snapshot(&self, now_secs: u64) -> MarmotSnapshot {
        self.projection()
            .map(|projection| projection.snapshot(now_secs))
            .unwrap_or_else(MarmotSnapshot::empty)
    }

    #[must_use]
    pub(crate) fn messages(&self) -> Vec<messages_fb::GroupMessages> {
        self.projection()
            .map(|projection| projection.messages_all_groups(MARMOT_MESSAGE_PAGE))
            .unwrap_or_default()
    }

    fn clear_active(&self) {
        let previous = self.active.lock().ok().and_then(|mut active| active.take());
        if let Some(previous) = previous {
            self.drop_projection_interests(previous);
        }
    }

    fn replace_active(
        &self,
        pubkey_hex: String,
        projection: Arc<MarmotProjection>,
    ) -> Option<ActiveMarmotProjection> {
        self.active.lock().ok().and_then(|mut active| {
            active.replace(ActiveMarmotProjection {
                pubkey_hex,
                projection,
            })
        })
    }

    fn drop_projection_interests(&self, projection: ActiveMarmotProjection) {
        self.drop_giftwrap_inbox(&projection.pubkey_hex);
        for identity in projection.projection.group_message_identities() {
            let _ = self.actor_sender.send(ActorCommand::Interests(
                InterestsCommand::DropInterestOwner(identity),
            ));
        }
    }

    fn subscribe_giftwrap_inbox(&self, pubkey_hex: &str) {
        let _ = self
            .actor_sender
            .send(ActorCommand::Interests(InterestsCommand::EnsureInterest {
                identity: giftwrap_inbox_identity(pubkey_hex),
                interest: giftwrap_inbox_interest(pubkey_hex),
            }));
    }

    fn drop_giftwrap_inbox(&self, pubkey_hex: &str) {
        let _ = self.actor_sender.send(ActorCommand::Interests(
            InterestsCommand::DropInterestOwner(giftwrap_inbox_identity(pubkey_hex)),
        ));
    }
}

/// Install active Marmot support on a host composition root.
pub fn install(
    app: &mut (impl ActionRegistrar
              + HostCapabilities
              + IdentityChangeRegistrar
              + IngestParserRegistrar
              + SnapshotProjectionRegistrar),
    config: MarmotConfig,
) -> Result<(), MarmotInstallError> {
    let runtime = Arc::new(MarmotRuntime::new(config, app.actor_sender()));

    app.register_action(MarmotActionModule::new(Arc::clone(&runtime)))?;

    for kind in MARMOT_INGEST_KINDS {
        app.replace_ingest_parser(
            kind,
            MARMOT_INGEST_SLOT,
            Arc::new(MarmotIngestParser::new(Arc::clone(&runtime))) as Arc<dyn IngestParser>,
        );
    }

    let runtime_for_projection = Arc::clone(&runtime);
    app.register_typed_snapshot_projection_with_time(
        snapshot_fb::PROJECTION_KEY,
        move |now_secs| {
            Some(snapshot_fb::typed_projection(
                &runtime_for_projection.snapshot(now_secs),
            ))
        },
    );

    let runtime_for_messages = Arc::clone(&runtime);
    app.register_typed_snapshot_projection(messages_fb::PROJECTION_KEY, move || {
        Some(messages_fb::typed_projection(
            &runtime_for_messages.messages(),
        ))
    });

    let runtime_for_identity = Arc::clone(&runtime);
    app.register_identity_change_observer(move |active_pubkey| {
        runtime_for_identity.rebind_for_identity(active_pubkey);
    });

    let initial_identity = app
        .active_pubkey()
        .lock()
        .ok()
        .and_then(|slot| slot.clone());
    runtime.rebind_for_identity(initial_identity);
    Ok(())
}

#[cfg(test)]
#[path = "runtime_tests.rs"]
mod tests;
