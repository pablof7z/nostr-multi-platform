use std::collections::BTreeSet;

use crate::live::LiveKernelSink;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ProfileClaim {
    pub pubkey: String,
    pub consumer_id: String,
    pub shape: ProfileClaimShape,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ProfileClaimShape {
    Ref,
    Card,
}

#[derive(Default)]
pub struct VisibleProfileClaims {
    active: BTreeSet<ProfileClaim>,
}

impl VisibleProfileClaims {
    pub fn reconcile(&mut self, sink: &LiveKernelSink, current: BTreeSet<ProfileClaim>) {
        for claim in self.active.difference(&current) {
            let same_consumer_still_visible = current
                .iter()
                .any(|next| next.pubkey == claim.pubkey && next.consumer_id == claim.consumer_id);
            if same_consumer_still_visible {
                continue;
            }
            sink.release_ref(&claim.pubkey, &claim.consumer_id);
        }
        self.active = current;
    }
}
