#[doc(hidden)]
#[macro_export]
macro_rules! __nmp_exclusive_claim_symbol {
    (
        true,
        $owner_id:literal,
        $id:literal,
        $claim_type:literal,
        $scope_kind:literal,
        $scope_value:literal,
        $context:literal
    ) => {
        const _: () = {
            #[used]
            #[unsafe(export_name = concat!(
                                                "__nmp_own__",
                                                $claim_type,
                                                "__",
                                                $scope_kind,
                                                "__",
                                                $scope_value,
                                                "__",
                                                $context
                                            ))]
            static CLAIM: $crate::ExclusiveClaimSymbol =
                $crate::ExclusiveClaimSymbol::new($owner_id, $id);
        };
    };
    (
        false,
        $owner_id:literal,
        $id:literal,
        $claim_type:literal,
        $scope_kind:literal,
        $scope_value:literal,
        $context:literal
    ) => {};
}

/// Declare a crate's positive ownership descriptor.
///
/// The macro intentionally accepts a small literal-only grammar so tooling can
/// derive reports from active workspace packages without a hand-maintained
/// central registry.
#[macro_export]
macro_rules! declare_crate_ownership {
    (
        owner_id: $owner_id:literal,
        crate_name: $crate_name:literal,
        summary: $summary:literal,
        claims: [
            $(
                {
                    claim_type: $claim_type:literal,
                    id: $id:literal,
                    exclusive: $exclusive:tt,
                    scope: {
                        kind: $scope_kind:literal,
                        value: $scope_value:literal,
                        context: $context:literal $(,)?
                    },
                    owns: [ $( $owns:literal ),* $(,)? ] $(,)?
                }
            ),* $(,)?
        ],
        notes: [
            $(
                {
                    claim: $note_claim:literal,
                    text: $note_text:literal $(,)?
                }
            ),* $(,)?
        ] $(,)?
    ) => {
        /// Compiled ownership descriptor for this crate.
        pub const OWNERSHIP: $crate::CrateOwnershipDescriptor =
            $crate::CrateOwnershipDescriptor {
                owner_id: $owner_id,
                crate_name: $crate_name,
                summary: $summary,
                claims: &[
                    $(
                        $crate::OwnershipClaim {
                            claim_type: $claim_type,
                            id: $id,
                            exclusive: $exclusive,
                            scope_kind: $scope_kind,
                            scope_value: $scope_value,
                            context: $context,
                            owns: &[ $( $owns ),* ],
                        }
                    ),*
                ],
                notes: &[
                    $(
                        $crate::OwnershipNote {
                            claim: $note_claim,
                            text: $note_text,
                        }
                    ),*
                ],
            };

        /// Return this crate's compiled ownership descriptor.
        #[must_use]
        pub const fn ownership_descriptor() -> &'static $crate::CrateOwnershipDescriptor {
            &OWNERSHIP
        }

        $(
            $crate::__nmp_exclusive_claim_symbol!(
                $exclusive,
                $owner_id,
                $id,
                $claim_type,
                $scope_kind,
                $scope_value,
                $context
            );
        )*
    };
}
