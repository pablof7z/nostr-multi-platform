package org.nmp.android

import kotlinx.coroutines.flow.MutableSharedFlow
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import org.nmp.android.model.ProfileCard
import org.nmp.android.ui.KernelProfileHost

/**
 * Claim-churn regression guard for [KernelProfileHost] (#1294; mirrors the
 * chirp-web fix 4d1888f9a).
 *
 * The host must be a *stable* object across snapshot ticks: `rememberKernelProfileHost`
 * keys `remember(model)` and reads the per-key `refs.profile` keyed-ref cache
 * through a `cardProvider` lambda (ADR-0063 Lane G). A per-tick host instance
 * previously churned the registry components' resolve/release `DisposableEffect`
 * — `release → re-resolve` every tick — and each response triggered another tick,
 * an infinite loop.
 *
 * This is a pure JUnit test (no Compose runtime): it constructs the host directly
 * with a mutable backing var standing in for the keyed-ref cache, and verifies the
 * provider lambda lets one stable host read the latest card after the var changes
 * — without ever constructing a new host. `resolveFn` is wired to a counter to
 * assert that simply reading a profile never resolves; resolving is the
 * component's `DisposableEffect` responsibility, not the read's.
 *
 * [KernelProfileHost.resolve] is the pure (non-`@Composable`) read backing the
 * `@Composable profileForPubkey`; testing it exercises the same cache indirection.
 */
class KernelProfileHostStabilityTest {

    private val pubkey = "a".repeat(64)

    private fun card(displayName: String?): ProfileCard =
        ProfileCard(pubkey = pubkey, displayName = displayName, nip05 = "", about = "")

    @Test
    fun profileForPubkey_returnsUpdatedProfiles_withoutRecreatingSelf() {
        // Mutable backing var simulating successive keyed-ref cache states. The
        // host is built ONCE and never rebuilt; only the var the provider closes
        // over moves.
        var backing: ProfileCard? = card("Alice")

        var resolves = 0
        var releases = 0
        val host = KernelProfileHost(
            cardProvider = { backing },
            rowChanges = MutableSharedFlow(),
            resolveFn = { _, _, _, _ -> resolves++ },
            releaseFn = { _, _ -> releases++ },
            npubFor = { "npub1$it" },
        )

        // Tick 1: reads the initial card.
        assertEquals("Alice", host.resolve(pubkey)?.displayName)

        // Tick 2: kind:0 updates — a NEW card, same stable host. The provider
        // lambda sees the latest value with no new host construction.
        backing = card("Alice Updated")
        assertEquals("Alice Updated", host.resolve(pubkey)?.displayName)

        // Tick 3: pubkey drops out of the cache → null, again no rebuild.
        backing = null
        assertNull(host.resolve(pubkey))

        // Reading the cache never resolves or releases — that is the component's
        // DisposableEffect job. Multiple ticks must not drive claim churn.
        assertEquals("resolve must never resolve_ref", 0, resolves)
        assertEquals("resolve must never release", 0, releases)
    }
}
