//! Single decision site for appending the substrate-generic `outbound_public_tags`
//! to an outbound event's tag list. The ONLY place both publish arms
//! (`publish_unsigned_event` Auto + `publish_unsigned_event_to_relays` Explicit)
//! consult — so the kind→attach decision cannot drift (D11 one-door). The kernel
//! holds the tags as opaque `Vec<Vec<String>>`; this helper appends them only for
//! `PublishBehavior::PublicRoutable` kinds (notes, reactions, addressables). It
//! NEVER appends on Private (gift-wrap 1059 / sealed-DM 14), Reserved (kind:0/3/…),
//! or DiscoveryIndexable (relay lists) kinds.

use crate::kernel::Kernel;
use crate::publish::policy::{classify_publish_behavior, PublishBehavior};

/// Append the kernel's `outbound_public_tags` to `tags` iff `kind` is
/// `PublicRoutable`. No-op otherwise (and a no-op when the kernel carries no
/// tags). Idempotent intent: callers pass the event's own tag rows.
pub(crate) fn finalize_outbound_tags(kind: u32, tags: &mut Vec<Vec<String>>, kernel: &Kernel) {
    if classify_publish_behavior(kind) != PublishBehavior::PublicRoutable {
        return;
    }
    for extra in kernel.outbound_public_tags() {
        tags.push(extra.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::Kernel;

    #[test]
    fn appends_on_kind1_public_routable() {
        let mut kernel = Kernel::testing_new(16);
        kernel.set_outbound_public_tags(vec![vec!["client".into(), "Chirp".into()]]);

        let mut tags = vec![];
        finalize_outbound_tags(1, &mut tags, &kernel);

        assert_eq!(tags, vec![vec!["client".to_string(), "Chirp".to_string()]]);
    }

    #[test]
    fn appends_on_addressable_30023() {
        let mut kernel = Kernel::testing_new(16);
        kernel.set_outbound_public_tags(vec![vec!["client".into(), "Chirp".into()]]);

        let mut tags = vec![];
        finalize_outbound_tags(30023, &mut tags, &kernel);

        assert_eq!(tags, vec![vec!["client".to_string(), "Chirp".to_string()]]);
    }

    #[test]
    fn not_appended_on_giftwrap_1059() {
        let mut kernel = Kernel::testing_new(16);
        kernel.set_outbound_public_tags(vec![vec!["client".into(), "Chirp".into()]]);

        let mut tags: Vec<Vec<String>> = vec![];
        finalize_outbound_tags(1059, &mut tags, &kernel);

        assert_eq!(tags, Vec::<Vec<String>>::new());
    }

    #[test]
    fn not_appended_on_chat_14() {
        let mut kernel = Kernel::testing_new(16);
        kernel.set_outbound_public_tags(vec![vec!["client".into(), "Chirp".into()]]);

        let mut tags: Vec<Vec<String>> = vec![];
        finalize_outbound_tags(14, &mut tags, &kernel);

        assert_eq!(tags, Vec::<Vec<String>>::new());
    }

    #[test]
    fn not_appended_on_reserved_profile_0() {
        let mut kernel = Kernel::testing_new(16);
        kernel.set_outbound_public_tags(vec![vec!["client".into(), "Chirp".into()]]);

        let mut tags: Vec<Vec<String>> = vec![];
        finalize_outbound_tags(0, &mut tags, &kernel);

        assert_eq!(tags, Vec::<Vec<String>>::new());
    }

    #[test]
    fn not_appended_on_discovery_10002() {
        let mut kernel = Kernel::testing_new(16);
        kernel.set_outbound_public_tags(vec![vec!["client".into(), "Chirp".into()]]);

        let mut tags: Vec<Vec<String>> = vec![];
        finalize_outbound_tags(10002, &mut tags, &kernel);

        assert_eq!(tags, Vec::<Vec<String>>::new());
    }

    #[test]
    fn noop_when_kernel_has_no_tags() {
        let kernel = Kernel::testing_new(16);
        // kernel has default (empty) outbound_public_tags

        let mut tags: Vec<Vec<String>> = vec![];
        finalize_outbound_tags(1, &mut tags, &kernel);

        assert_eq!(tags, Vec::<Vec<String>>::new());
    }

    #[test]
    fn preserves_existing_tags() {
        let mut kernel = Kernel::testing_new(16);
        kernel.set_outbound_public_tags(vec![vec!["client".into(), "Chirp".into()]]);

        let mut tags = vec![vec!["e".to_string(), "abc".to_string()]];
        finalize_outbound_tags(1, &mut tags, &kernel);

        assert_eq!(
            tags,
            vec![
                vec!["e".to_string(), "abc".to_string()],
                vec!["client".to_string(), "Chirp".to_string()]
            ]
        );
    }
}
