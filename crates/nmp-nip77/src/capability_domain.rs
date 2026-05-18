//! Persistent per-relay capability rows.
//!
//! Exposes [`CapabilityDomain`] as a [`DomainModule`] so the kernel store
//! keeps an LMDB sub-database of `(relay_url, RelayCapabilities)` pairs
//! across launches.  This is the durable backing for the in-memory
//! [`crate::capability::InMemoryCapabilityCache`].
//!
//! ## Schema
//!
//! `CapabilityRow` is the on-disk record:
//!
//! ```json
//! { "relay_url": "wss://r.example/", "supports_nip77": true, "updated_at_s": 1700000000 }
//! ```
//!
//! Encoded as serde-JSON for human-readability in `nmp dump`; the schema is
//! intentionally narrow — capability ground truth lives in this module only,
//! per D4 (single writer per fact).

use nmp_core::substrate::{
    DomainIndex, DomainMigration, DomainModule, DomainRegistry,
};
use serde::{Deserialize, Serialize};

use crate::capability::{CapabilityCache, RelayCapabilities};

/// The on-disk record persisted per relay URL.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CapabilityRow {
    pub relay_url: String,
    pub supports_nip77: bool,
    pub updated_at_s: u64,
}

impl CapabilityRow {
    pub fn capabilities(&self) -> RelayCapabilities {
        RelayCapabilities {
            supports_nip77: self.supports_nip77,
        }
    }
}

/// `DomainModule` that owns the capability sub-database.
pub struct CapabilityDomain;

impl DomainModule for CapabilityDomain {
    const NAMESPACE: &'static str = "nmp.nip77.capabilities";
    const SCHEMA_VERSION: u32 = 1;

    fn migrations() -> Vec<DomainMigration> {
        Vec::new()
    }

    fn indexes() -> Vec<DomainIndex> {
        vec![DomainIndex {
            name: "by_supports_nip77",
            key_fn: |bytes| {
                serde_json::from_slice::<CapabilityRow>(bytes)
                    .ok()
                    .map(|row| row.supports_nip77.to_string().into_bytes())
            },
        }]
    }

    fn register(registry: &mut DomainRegistry) {
        registry.register_record::<CapabilityRow>();
    }
}

/// Hydrate an in-memory cache from a previously-persisted set of rows.
///
/// Typical pre-startup flow: open the LMDB store, scan the capability
/// sub-database, call [`hydrate_cache`] with the iterator.  The cache is
/// then ready before any reconciliation runs.
pub fn hydrate_cache<I>(cache: &dyn CapabilityCache, rows: I)
where
    I: IntoIterator<Item = CapabilityRow>,
{
    for row in rows {
        cache.set(&row.relay_url, row.capabilities());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::InMemoryCapabilityCache;

    #[test]
    fn schema_metadata_is_stable() {
        assert_eq!(CapabilityDomain::NAMESPACE, "nmp.nip77.capabilities");
        assert_eq!(CapabilityDomain::SCHEMA_VERSION, 1);
        assert!(CapabilityDomain::migrations().is_empty());
        assert_eq!(CapabilityDomain::indexes().len(), 1);
    }

    #[test]
    fn index_key_fn_extracts_supports_nip77() {
        let row = CapabilityRow {
            relay_url: "wss://r/".into(),
            supports_nip77: true,
            updated_at_s: 1,
        };
        let bytes = serde_json::to_vec(&row).unwrap();
        let idx = &CapabilityDomain::indexes()[0];
        assert_eq!((idx.key_fn)(&bytes), Some(b"true".to_vec()));
    }

    #[test]
    fn hydrate_cache_populates_each_row() {
        let cache = InMemoryCapabilityCache::new();
        hydrate_cache(
            &cache,
            vec![
                CapabilityRow {
                    relay_url: "wss://a/".into(),
                    supports_nip77: true,
                    updated_at_s: 1,
                },
                CapabilityRow {
                    relay_url: "wss://b/".into(),
                    supports_nip77: false,
                    updated_at_s: 2,
                },
            ],
        );
        assert_eq!(
            cache.get("wss://a/"),
            Some(RelayCapabilities {
                supports_nip77: true
            })
        );
        assert_eq!(
            cache.get("wss://b/"),
            Some(RelayCapabilities {
                supports_nip77: false
            })
        );
    }
}
