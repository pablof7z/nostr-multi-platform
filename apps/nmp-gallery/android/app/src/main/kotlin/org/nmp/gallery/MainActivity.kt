package org.nmp.gallery

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.viewModels
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.ui.Modifier
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import org.nmp.gallery.bridge.GalleryModel
import org.nmp.gallery.navigation.GalleryNavigation
import org.nmp.gallery.registry.ExternalSignerCapabilityBridge
import org.nmp.gallery.registry.LocalNostrProfileHost
import org.nmp.gallery.registry.NostrProfileHost

/**
 * Single-activity host for the gallery. Wires the [GalleryModel] (which
 * owns the kernel) into the component host bridge. Pages pass references;
 * registry components own their claim/release lifecycle.
 */
class MainActivity : ComponentActivity() {
    private val model: GalleryModel by viewModels()

    /**
     * ADR-0048 Stage 2 — D7 host adapter for the `external_signer`
     * capability. Owns the Activity Result launcher (registered in
     * `onCreate`, before first `onStart`); raw results route back to Rust
     * via [GalleryModel.deliverSignerResponse].
     */
    private lateinit var signerBridge: ExternalSignerCapabilityBridge

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        signerBridge = ExternalSignerCapabilityBridge(this) { responseJson ->
            model.deliverSignerResponse(responseJson)
        }
        signerBridge.register()
        model.registerExternalSignerHandler { requestJson ->
            signerBridge.handleJson(requestJson)
        }
        setContent {
            val profiles by model.profileMap.collectAsStateWithLifecycle()
            val latestProfiles = rememberUpdatedState(profiles)
            val profileHost = remember(model) {
                object : NostrProfileHost {
                    @Composable
                    override fun profileForPubkey(pubkey: String) = latestProfiles.value[pubkey]

                    override fun resolveProfileRef(pubkey: String, consumerId: String) {
                        model.resolveProfileRef(pubkey, consumerId)
                    }

                    override fun releaseProfileRef(pubkey: String, consumerId: String) {
                        model.releaseProfileRef(pubkey, consumerId)
                    }
                }
            }

            MaterialTheme {
                CompositionLocalProvider(LocalNostrProfileHost provides profileHost) {
                    Surface(modifier = Modifier.fillMaxSize()) {
                        GalleryNavigation(model = model)
                    }
                }
            }
        }
    }

    override fun onDestroy() {
        model.unregisterExternalSignerHandler()
        signerBridge.unregister()
        super.onDestroy()
    }
}
