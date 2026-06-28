package org.nmp.gallery.registry

import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider

/**
 * App-root provider for NMP registry components.
 *
 * The provider installs the existing profile and embed CompositionLocals in one
 * place. Apps still own the concrete bridge objects: profile data comes from
 * `refs.profile`, event envelopes come from the Rust-derived
 * `refs.event.envelopes` sidecar, and components only render plus manage
 * visible resolve/release lifecycle.
 */
@Composable
fun NmpComponentHostProvider(
    profileHost: NostrProfileHost?,
    resolvedEventEmbeds: Map<String, EmbeddedEventEnvelope>,
    eventRefResolver: EventRefResolver? = null,
    kindRegistry: NostrKindRegistry = NostrKindRegistry.makeDefault(),
    content: @Composable () -> Unit,
) {
    CompositionLocalProvider(
        LocalNostrProfileHost provides profileHost,
        LocalResolvedEventEmbeds provides resolvedEventEmbeds,
        LocalEventRefResolver provides eventRefResolver,
        LocalNostrKindRegistry provides kindRegistry,
        content = content,
    )
}
