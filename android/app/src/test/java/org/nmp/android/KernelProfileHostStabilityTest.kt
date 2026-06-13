package org.nmp.android

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
 * keys `remember(model)` and threads the per-tick profiles map in through a
 * `profilesProvider` lambda (backed by `rememberUpdatedState`). A per-tick host
 * instance previously churned the registry components' claim/release
 * `DisposableEffect` — `release → re-claim` every tick — and each claim response
 * triggered another tick, an infinite loop.
 *
 * This is a pure JUnit test (no Compose runtime): it constructs the host directly
 * with a mutable backing var standing in for the snapshot map, and verifies the
 * provider lambda lets one stable host read the latest profiles after the var
 * changes — without ever constructing a new host. `claimFn` is wired to a counter
 * to assert that simply reading a profile never claims; claiming is the
 * component's `DisposableEffect` responsibility, not the read's.
 *
 * [KernelProfileHost.resolve] is the pure (non-`@Composable`) read backing the
 * `@Composable profileForPubkey`; testing it exercises the same map indirection.
 */
class KernelProfileHostStabilityTest {

    private val pubkey = "a".repeat(64)

    private fun card(displayName: String?): ProfileCard =
        ProfileCard(pubkey = pubkey, displayName = displayName, nip05 = "", about = "")

    @Test
    fun profileForPubkey_returnsUpdatedProfiles_withoutRecreatingSelf() {
        // Mutable backing var simulating successive snapshot ticks. The host is
        // built ONCE and never rebuilt; only the var the provider closes over moves.
        var backing: Map<String, ProfileCard> = mapOf(pubkey to card("Alice"))

        var claims = 0
        var releases = 0
        val host = KernelProfileHost(
            profilesProvider = { backing },
            claimFn = { _, _ -> claims++ },
            releaseFn = { _, _ -> releases++ },
            npubFor = { "npub1$it" },
        )

        // Tick 1: reads the initial map.
        assertEquals("Alice", host.resolve(pubkey)?.displayName)

        // Tick 2: kind:0 updates — a NEW map object, same stable host. The provider
        // lambda sees the latest value with no new host construction.
        backing = mapOf(pubkey to card("Alice Updated"))
        assertEquals("Alice Updated", host.resolve(pubkey)?.displayName)

        // Tick 3: pubkey drops out of the projection → null, again no rebuild.
        backing = emptyMap()
        assertNull(host.resolve(pubkey))

        // Reading the projection never claims or releases — that is the component's
        // DisposableEffect job. Multiple ticks must not drive claim churn.
        assertEquals("resolve must never claim", 0, claims)
        assertEquals("resolve must never release", 0, releases)
    }
}
