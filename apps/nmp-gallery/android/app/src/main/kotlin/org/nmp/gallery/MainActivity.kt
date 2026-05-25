package org.nmp.gallery

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.viewModels
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.ui.Modifier
import org.nmp.gallery.bridge.GalleryModel
import org.nmp.gallery.navigation.GalleryNavigation

/**
 * Single-activity host for the gallery. Wires the [GalleryModel] (which
 * owns the kernel) into the nav graph and otherwise stays out of the way.
 */
class MainActivity : ComponentActivity() {
    private val model: GalleryModel by viewModels()

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContent {
            MaterialTheme {
                Surface(modifier = Modifier.fillMaxSize()) {
                    GalleryNavigation(model = model)
                }
            }
        }
    }
}
