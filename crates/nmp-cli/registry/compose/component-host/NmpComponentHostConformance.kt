package org.nmp.registry

import androidx.compose.runtime.Composable
import nmp.content.EmbedKindProjection
import nmp.content.EmbeddedEventEnvelope
import nmp.content.EventRefResolver
import nmp.content.ShortNoteProjection

/**
 * In-memory rows for component-host conformance tests and previews.
 *
 * These values model the host contract without a live kernel:
 * - `refs.profile` is the profile row source.
 * - `refs.event` is the authoritative event-ref row source.
 * - `refs.event.envelopes` is derived render data for embeds.
 *
 * App tests can mount [NmpComponentHostConformanceHarness] around registry
 * components and assert that the component reads these host values instead of
 * reaching for a kernel handle or ABI/runtime object.
 */
object NmpComponentHostConformanceFixture {
    const val REFS_PROFILE_KEY = "refs.profile"
    const val REFS_EVENT_KEY = "refs.event"
    const val REFS_EVENT_ENVELOPES_KEY = "refs.event.envelopes"

    const val PUBKEY = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    const val PRIMARY_EVENT_ID = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
    const val EVENT_URI = "nostr:nevent1componenthost"

    val profile = ProfileWire(
        pubkey = PUBKEY,
        displayName = "Conformance Alice",
        about = "Profile row supplied by refs.profile.",
        pictureUrl = "https://example.invalid/alice.png",
        nip05 = "alice@example.invalid",
        npub = "npub1componenthostfixture",
        npubShort = "npub1component...fixture",
    )

    val eventEnvelope = EmbeddedEventEnvelope(
        uri = EVENT_URI,
        primaryId = PRIMARY_EVENT_ID,
        projection = EmbedKindProjection(
            shortNote = ShortNoteProjection(
                id = PRIMARY_EVENT_ID,
                authorPubkey = PUBKEY,
                createdAt = 1_700_000_000,
                content = "Event render data supplied by refs.event.envelopes.",
            ),
        ),
    )

    val envelopesByPrimaryId = mapOf(
        PRIMARY_EVENT_ID to eventEnvelope,
        EVENT_URI to eventEnvelope,
    )

    val expectedKeys = listOf(REFS_PROFILE_KEY, REFS_EVENT_KEY, REFS_EVENT_ENVELOPES_KEY)
}

class FixtureNostrProfileHost : NostrProfileHost {
    val resolved = mutableListOf<Pair<String, String>>()
    val released = mutableListOf<Pair<String, String>>()

    @Composable
    override fun profileForPubkey(pubkey: String): ProfileWire? =
        if (pubkey == NmpComponentHostConformanceFixture.PUBKEY) {
            NmpComponentHostConformanceFixture.profile
        } else {
            null
        }

    override fun resolveProfileRef(pubkey: String, consumerId: String) {
        resolved += pubkey to consumerId
    }

    override fun releaseProfileRef(pubkey: String, consumerId: String) {
        released += pubkey to consumerId
    }
}

class FixtureEventRefResolver : EventRefResolver {
    val resolved = mutableListOf<Pair<String, String>>()
    val released = mutableListOf<Pair<String, String>>()

    override fun resolveEventRef(uri: String, consumerId: String) {
        resolved += uri to consumerId
    }

    override fun releaseEventRef(uri: String, consumerId: String) {
        released += uri to consumerId
    }
}

@Composable
fun NmpComponentHostConformanceHarness(
    profileHost: NostrProfileHost = FixtureNostrProfileHost(),
    eventRefResolver: EventRefResolver = FixtureEventRefResolver(),
    content: @Composable () -> Unit,
) {
    NmpComponentHostProvider(
        profileHost = profileHost,
        resolvedEventEmbeds = NmpComponentHostConformanceFixture.envelopesByPrimaryId,
        eventRefResolver = eventRefResolver,
        content = content,
    )
}
