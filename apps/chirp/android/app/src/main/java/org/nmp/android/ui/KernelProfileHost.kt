package org.nmp.android.ui

import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.LaunchedEffect
import kotlinx.coroutines.flow.SharedFlow
import org.nmp.android.KernelModel
import org.nmp.android.KeyedRowChange
import org.nmp.android.RefLiveness
import org.nmp.android.RefShape
import org.nmp.android.components.LocalNostrProfileHost
import org.nmp.android.components.NostrProfileHost
import org.nmp.android.components.ProfileWire
import org.nmp.android.model.ProfileCard

/**
 * D7 host adapter bridging the registry [NostrProfileHost] component contract to
 * Chirp's Rust kernel. The registry profile components (NostrAvatar /
 * NostrProfileName / NostrNip05Badge) are *self-claiming*: handed a pubkey they
 * resolve it, read the resolved kind:0, and release on dispose. This adapter is
 * the seam those components call through.
 *
 * ADR-0063 Lane G (#1671): profiles are resolved + read EXCLUSIVELY via the
 * unified `resolve_ref` (Profile namespace) + the per-key `refs.profile`
 * keyed-ref cache — NOT the legacy `claim_profile` / `resolved_profiles` /
 * `claimed_profiles` whole-map path, and with NO app-side profile cache (D4).
 * [profileForPubkey] re-reads the cache per-key when that pubkey's row changes
 * ([profileRowChanged]), so exactly one avatar/name leaf re-renders on one
 * pubkey's kind:0 — per-key reactivity, not a whole-map StateFlow broadcast (the
 * iOS `KeyedRefRowObserver` twin).
 *
 * Construct once per screen scope via [rememberKernelProfileHost], then provide
 * it over [LocalNostrProfileHost].
 *
 * @param cardProvider per-key TYPED read of the `refs.profile` keyed-ref cache
 *   (`model.profileCard(pubkey)`), keyed by 64-hex pubkey. The source of truth
 *   (D4); returns null until the kernel resolves the kind:0.
 * @param rowChanges the kernel's per-key `refs.profile` row-change flow; a leaf
 *   observes ONLY its own pubkey to trigger a re-read.
 * @param npubFor host-side NIP-19 encoder (`nmp_app_encode_profile`); the Rust
 *   side owns the canonical identifier (ADR-0032 / V-115).
 */
class KernelProfileHost(
    private val cardProvider: (pubkey: String) -> ProfileCard?,
    private val rowChanges: SharedFlow<KeyedRowChange>,
    private val resolveFn: (pubkey: String, consumerId: String, shape: RefShape, liveness: RefLiveness) -> Unit,
    private val releaseFn: (pubkey: String, consumerId: String) -> Unit,
    private val npubFor: (pubkey: String) -> String?,
) : NostrProfileHost {

    @Composable
    override fun profileForPubkey(pubkey: String): ProfileWire? {
        // Per-key observability (ADR-0063 D4): re-read the keyed-ref cache only
        // when THIS pubkey's row changes — not on every snapshot tick and not via
        // a whole-map StateFlow. A `version` tick keyed on the pubkey re-runs the
        // read; the cache is the source of truth (no app-side cache).
        var version by remember(pubkey) { mutableStateOf(0) }
        LaunchedEffect(pubkey) {
            rowChanges.collect { change ->
                if (change.rowKey == pubkey) version++
            }
        }
        // Keying the read on `version` re-runs it (and recomposes this leaf) only
        // when THIS pubkey's row commits — the per-key reactive read.
        return remember(pubkey, version) { resolve(pubkey) }
    }

    /**
     * Pure (non-`@Composable`) per-key read backing [profileForPubkey]. Reads the
     * `refs.profile` keyed-ref cache directly (the source of truth, D4). Extracted
     * from the composable so the read contract is unit-testable without a Compose
     * runtime.
     */
    fun resolve(pubkey: String): ProfileWire? {
        val card = cardProvider(pubkey) ?: return null
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

    // ADR-0063 Lane G: the self-claiming registry components are feed/list/avatar
    // surfaces, so they resolve the small ProfileRef shape with CacheOk liveness
    // (background fill, no tailing sub). The open profile screen resolves the full
    // ProfileCard/Live shape itself (see ProfileScreen).
    override fun resolveProfileRef(pubkey: String, consumerId: String) =
        resolveFn(pubkey, consumerId, RefShape.ProfileRef, RefLiveness.CacheOk)

    override fun releaseProfileRef(pubkey: String, consumerId: String) = releaseFn(pubkey, consumerId)
}

/**
 * Provide a [KernelProfileHost] over [LocalNostrProfileHost] for the registry
 * profile components. Single binding for every screen so the call sites (timeline,
 * profile, DM) wire the components identically.
 *
 * The [KernelProfileHost] instance is **stable** — keyed on [model] alone — and
 * reads the per-key `refs.profile` cache through [model.profileCard]. A stable
 * host is essential: the registry components key their resolve/release
 * [androidx.compose.runtime.DisposableEffect] on the host, so a per-tick host
 * churned `release → re-resolve` every tick (#1294). Components recompose per-key
 * via [model.profileRowChanged] (push, no polling — D8).
 */
@Composable
fun rememberKernelProfileHost(
    model: KernelModel,
): KernelProfileHost {
    return remember(model) {
        KernelProfileHost(
            cardProvider = { pubkey -> model.profileCard(pubkey) },
            rowChanges = model.profileRowChanged,
            resolveFn = { pubkey, consumerId, shape, liveness ->
                model.resolveProfile(pubkey, consumerId, shape, liveness)
            },
            releaseFn = { pubkey, consumerId -> model.releaseProfile(pubkey, consumerId) },
            npubFor = { pubkey -> model.encodeProfile(pubkey) },
        )
    }
}
