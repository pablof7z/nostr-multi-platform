use flatbuffers::WIPOffset;

use super::{fb, malformed};
use crate::publish::action::{
    PublishRouteClass, PublishSigner, PublishSignerProvenance, PublishTarget, RelayUrl,
};
use crate::substrate::ActionPayloadDecodeError;

pub(super) fn build_target<'a>(
    fbb: &mut flatbuffers::FlatBufferBuilder<'a>,
    target: &PublishTarget,
) -> WIPOffset<fb::PublishTarget<'a>> {
    let (explicit, route_class, relay_offsets) = match target {
        PublishTarget::Auto => (false, PublishRouteClass::ManualOverride, Vec::new()),
        PublishTarget::Explicit {
            relays,
            route_class,
        } => (
            true,
            *route_class,
            relays
                .iter()
                .map(|r| fbb.create_string(r))
                .collect::<Vec<_>>(),
        ),
    };
    let relays = fbb.create_vector(&relay_offsets);
    let route_class = fbb.create_string(route_class.wire_token());
    fb::PublishTarget::create(
        fbb,
        &fb::PublishTargetArgs {
            explicit,
            relays: Some(relays),
            route_class: Some(route_class),
        },
    )
}

pub(super) fn read_target(
    target: fb::PublishTarget<'_>,
) -> Result<PublishTarget, ActionPayloadDecodeError> {
    if !target.explicit() {
        return Ok(PublishTarget::Auto);
    }
    let relays: Vec<RelayUrl> = target
        .relays()
        .map(|v| v.iter().map(|s| s.to_string()).collect())
        .unwrap_or_default();
    let route_class = target
        .route_class()
        .ok_or_else(|| malformed("explicit publish target missing route_class"))?;
    let route_class = PublishRouteClass::from_wire_token(route_class).ok_or_else(|| {
        malformed(format!(
            "unknown explicit publish route_class '{route_class}'"
        ))
    })?;
    Ok(PublishTarget::Explicit {
        relays,
        route_class,
    })
}

pub(super) fn build_signer<'a>(
    fbb: &mut flatbuffers::FlatBufferBuilder<'a>,
    signer: &PublishSigner,
) -> Option<WIPOffset<fb::PublishSignerSelection<'a>>> {
    match signer {
        PublishSigner::Active => None,
        PublishSigner::Registered { pubkey, provenance } => {
            let pubkey = fbb.create_string(pubkey);
            let provenance = fbb.create_string(provenance.wire_token());
            Some(fb::PublishSignerSelection::create(
                fbb,
                &fb::PublishSignerSelectionArgs {
                    mode: fb::PublishSignerMode::Registered,
                    pubkey: Some(pubkey),
                    provenance: Some(provenance),
                },
            ))
        }
    }
}

pub(super) fn read_signer(
    signer: Option<fb::PublishSignerSelection<'_>>,
) -> Result<PublishSigner, ActionPayloadDecodeError> {
    let Some(signer) = signer else {
        return Ok(PublishSigner::Active);
    };
    match signer.mode() {
        fb::PublishSignerMode::Active => Ok(PublishSigner::Active),
        fb::PublishSignerMode::Registered => {
            let pubkey = signer
                .pubkey()
                .ok_or_else(|| malformed("registered publish signer missing pubkey"))?
                .to_string();
            let provenance = signer
                .provenance()
                .ok_or_else(|| malformed("registered publish signer missing provenance"))?;
            let provenance =
                PublishSignerProvenance::from_wire_token(provenance).ok_or_else(|| {
                    malformed(format!(
                        "unknown registered publish signer provenance '{provenance}'"
                    ))
                })?;
            Ok(PublishSigner::registered(pubkey, provenance))
        }
        other => Err(malformed(format!(
            "unknown PublishSignerMode discriminant: {other:?}"
        ))),
    }
}
