//! Native diagnostic read helpers.

use nmp_core::projection_to_json;
use serde_json::json;

use crate::NmpApp;

pub const DOMAIN_ROUTING: i32 = 0;
pub const DOMAIN_COMPOSITION: i32 = 1;
pub const DOMAIN_MERGED: i32 = 2;

impl NmpApp {
    #[must_use]
    pub fn debug_info_json(&self, domain: i32) -> serde_json::Value {
        match domain {
            DOMAIN_ROUTING => self.routing_debug_json(),
            DOMAIN_COMPOSITION => self.composition_debug_json(),
            DOMAIN_MERGED => json!({
                "routing": self.routing_debug_json(),
                "composition": self.composition_debug_json(),
            }),
            _ => json!({}),
        }
    }

    fn routing_debug_json(&self) -> serde_json::Value {
        let Some(projection) = self.routing_trace() else {
            return empty_routing_value();
        };
        projection_to_json(&projection)
    }

    fn composition_debug_json(&self) -> serde_json::Value {
        self.composition_ledger().to_json()
    }
}

#[must_use]
pub fn empty_debug_info_json(domain: i32) -> serde_json::Value {
    match domain {
        DOMAIN_ROUTING => empty_routing_value(),
        DOMAIN_COMPOSITION => json!({
            "schema_version": nmp_core::COMPOSITION_REPORT_SCHEMA_VERSION,
            "count": 0,
            "records": [],
        }),
        DOMAIN_MERGED => json!({
            "routing": empty_routing_value(),
            "composition": {
                "schema_version": nmp_core::COMPOSITION_REPORT_SCHEMA_VERSION,
                "count": 0,
                "records": [],
            },
        }),
        _ => json!({}),
    }
}

fn empty_routing_value() -> serde_json::Value {
    json!({
        "schema_version": nmp_core::ROUTING_TRACE_SCHEMA_VERSION,
        "capacity": 0,
        "publishes": [],
        "subscriptions": [],
    })
}
