extension TypedProjectionGlue {
    /// Map the optional ADR-0051 NIP-11 relay metadata child table. Table
    /// presence is the discriminator (`row.info == nil` maps to JSON `null`);
    /// each `has_*` companion preserves the JSON `Option` parity.
    static func relayDiagnosticsInfo(
        _ info: nmp_kernel_RelayDiagnosticsInfo
    ) -> RelayDiagnosticsInfo {
        RelayDiagnosticsInfo(
            name: info.hasName ? (info.name ?? "") : nil,
            description: info.hasDescription ? (info.description ?? "") : nil,
            icon: info.hasIcon ? (info.icon ?? "") : nil,
            pubkey: info.hasPubkey ? (info.pubkey ?? "") : nil,
            contact: info.hasContact ? (info.contact ?? "") : nil,
            software: info.hasSoftware ? (info.software ?? "") : nil,
            version: info.hasVersion ? (info.version ?? "") : nil,
            supportedNips: info.supportedNips.map { $0 },
            paymentRequired: info.hasPaymentRequired ? info.paymentRequired : nil,
            authRequired: info.hasAuthRequired ? info.authRequired : nil,
            restrictedWrites: info.hasRestrictedWrites ? info.restrictedWrites : nil
        )
    }
}
