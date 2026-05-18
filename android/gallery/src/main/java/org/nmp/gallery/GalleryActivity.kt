package org.nmp.gallery

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import kotlinx.serialization.json.Json
import org.nmp.gallery.model.GalleryBundle
import org.nmp.gallery.ui.GalleryScreen

/**
 * Entry point for the kernel-free NMP Content Gallery app. Loads the
 * committed `content-gallery-bundle.json` from `assets/`, decodes via
 * kotlinx-serialization (with [Json.ignoreUnknownKeys] for forward
 * compatibility), and dispatches to [GalleryScreen] or [ErrorScreen].
 */
class GalleryActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val result = loadBundle()
        setContent {
            MaterialTheme {
                Surface(modifier = Modifier.fillMaxSize()) {
                    when (result) {
                        is BundleResult.Success -> GalleryScreen(result.value)
                        is BundleResult.Failure -> ErrorScreen(result.message)
                    }
                }
            }
        }
    }

    private fun loadBundle(): BundleResult = try {
        val text = assets.open(BUNDLE_ASSET)
            .bufferedReader(Charsets.UTF_8)
            .use { it.readText() }
        BundleResult.Success(BundleJson.decodeFromString(GalleryBundle.serializer(), text))
    } catch (e: Exception) {
        BundleResult.Failure("Decode failed: ${e.message ?: e::class.simpleName}")
    }

    private companion object {
        const val BUNDLE_ASSET = "content-gallery-bundle.json"
        val BundleJson: Json = Json {
            ignoreUnknownKeys = true
            isLenient = true
            classDiscriminator = "type"
        }
    }
}

/** Local Result-like sum type — avoids the stdlib `Result` shape mismatch. */
private sealed class BundleResult {
    data class Success(val value: GalleryBundle) : BundleResult()
    data class Failure(val message: String) : BundleResult()
}

@Composable
private fun ErrorScreen(message: String) {
    Column(
        modifier = Modifier
            .fillMaxSize()
            .padding(24.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp, Alignment.CenterVertically),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Text("⚠", style = MaterialTheme.typography.displayMedium)
        Text(
            "Bundle load failed",
            style = MaterialTheme.typography.titleMedium,
            fontWeight = FontWeight.Bold,
        )
        Text(
            message,
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}
