package org.nmp.android.ui

import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import org.nmp.android.KernelModel
import org.nmp.android.components.LocalNostrProfileHost
import org.nmp.android.components.NostrProfileHost
import org.nmp.android.components.ProfileWire
import org.nmp.android.model.ProfileCard

/**
 * D7 host adapter bridging the registry [NostrProfileHost] component contract to
 * Chirp's Rust kernel. The registry profile components (NostrAvatar /
 * NostrProfileName / NostrNip05Badge) are *self-claiming*: handed a pubkey they
 * issue a claim, read the resolved kind:0 from the snapshot projection, and
 * release on dispose. This adapter is the seam those components call through.
 *
 * Mirrors iOS `nostrProfileHost`: the kernel owns the kind:0 fetch policy
 * (claim → batch a REQ against the indexer / author NIP-65 write set). Kotlin
 * decides nothing — [profileForPubkey] is a pure read of the verbatim
 * `resolved_profiles` / `claimed_profiles` projection (thin-shell, D8); the
 * claim/release calls forward to [KernelModel] unchanged.
 *
 * Construct once per screen scope with the live [KernelModel], then provide it
 * via [ProvideKernelProfileHost] so the registry components resolve it from
 * [LocalNostrProfileHost].
 *
 * @param profilesProvider reads the kernel's current `resolved_profiles` map
 *   (already merged with any screen-local `claimed_profiles` override), keyed by
 *   64-hex pubkey. A *provider lambda* rather than a snapshot of the map: the
 *   host instance is stable across snapshot ticks (claim-churn fix, #1294),
 *   while [profileForPubkey] always reads the latest map by invoking it.
 * @param npubFor host-side NIP-19 encoder (`nmp_app_encode_profile`); returns
 *   the full `npub1…` / `nprofile1…` identifier or null when the kernel is
 *   unavailable. The Rust side owns the canonical identifier (ADR-0032 / V-115);
 *   Kotlin never reformats it.
 */
class KernelProfileHost(
    private val profilesProvider: () -> Map<String, ProfileCard>,
    private val claimFn: (pubkey: String, consumerId: String) -> Unit,
    private val releaseFn: (pubkey: String, consumerId: String) -> Unit,
    private val npubFor: (pubkey: String) -> String?,
) : NostrProfileHost {

    @Composable
    override fun profileForPubkey(pubkey: String): ProfileWire? = resolve(pubkey)

    /**
     * Pure (non-`@Composable`) projection read backing [profileForPubkey].
     *
     * Always reads the *latest* profiles map via [profilesProvider], so a single
     * stable host instance (keyed on the model, not the per-tick map — #1294)
     * still returns fresh data as snapshots arrive. Extracted from the composable
     * so the claim-churn stability contract is unit-testable without a Compose
     * runtime.
     */
    fun resolve(pubkey: String): ProfileWire? {
        val card = profilesProvider()[pubkey] ?: return null
        // The kernel ships the canonical npub host-side (ADR-0032 / V-115); the
        // projection no longer carries it. Encode once and abbreviate for the
        // short label exactly as the screens do, never re-deriving from hex.
        val npub = npubFor(pubkey).orEmpty()
        val npubShort = npub.takeIf { it.isNotEmpty() }?.let { shortHex(it) } ?: shortHex(pubkey)
        return ProfileWire(
            pubkey = pubkey,
            displayName = card.displayName?.takeIf { it.isNotEmpty() },
            about = card.about.takeIf { it.isNotEmpty() },
            pictureUrl = card.pictureUrl?.takeIf { it.isNotEmpty() },
            nip05 = card.nip05.takeIf { it.isNotEmpty() },
            npub = npub,
            npubShort = npubShort,
        )
    }

    override fun claimProfile(pubkey: String, consumerId: String) = claimFn(pubkey, consumerId)

    override fun releaseProfile(pubkey: String, consumerId: String) = releaseFn(pubkey, consumerId)
}

/**
 * Provide a [KernelProfileHost] over [LocalNostrProfileHost] for the registry
 * profile components, alongside the existing [LocalResolvedProfiles] /
 * [LocalProfileClaimer]. Single binding for every screen so the three call
 * sites (timeline, profile, DM) wire the components identically.
 *
 * [profiles] is the current snapshot's merged profile map. It changes (a new
 * `Map` object) on every snapshot tick, but the [KernelProfileHost] instance is
 * **stable** — keyed on [model] alone — and reads the latest map through a
 * [rememberUpdatedState]-backed provider. A stable host is essential: the
 * registry components key their claim/release [DisposableEffect] on the host, so
 * a per-tick host churned `release → re-claim` every tick, and each claim
 * response triggered another tick — an infinite claim-churn loop (#1294, mirrors
 * the chirp-web fix 4d1888f9a). Components still recompose when the kind:0
 * resolves because [profileForPubkey] reads the updated map (push, no polling —
 * D8).
 */
@Composable
fun rememberKernelProfileHost(
    model: KernelModel,
    profiles: Map<String, ProfileCard>,
): KernelProfileHost {
    // Latest profiles map, read by the stable host's provider lambda without
    // recreating the host on each snapshot tick.
    val latestProfiles = rememberUpdatedState(profiles)
    return remember(model) {
        KernelProfileHost(
            profilesProvider = { latestProfiles.value },
            claimFn = { pubkey, consumerId -> model.claimProfile(pubkey, consumerId) },
            releaseFn = { pubkey, consumerId -> model.releaseProfile(pubkey, consumerId) },
            npubFor = { pubkey -> model.encodeProfile(pubkey) },
        )
    }
}
