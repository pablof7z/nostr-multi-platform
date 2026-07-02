package org.nmp.gallery.registry

/**
 * Ordered list of signers the Android detector knows about (detection
 * precedence = list order). Consumed by `detectInstalledSigners` in
 * ExternalSignerWire.kt (same package). Every `intentScheme` here MUST also
 * appear in `<queries>` in AndroidManifest.xml.
 */
val KNOWN_NOSTR_SIGNERS: List<NostrSignerInfo> = listOf(
    NostrSignerInfo(
        displayName = "Amber",
        intentScheme = "nostrsigner",
        contentAuthority = "com.greenart7c3.nostrsigner",
        packageName = "com.greenart7c3.nostrsigner",
    ),
    NostrSignerInfo(
        displayName = "Primal",
        intentScheme = "primal",
        contentAuthority = null,
        packageName = "net.primal.android",
    ),
)
