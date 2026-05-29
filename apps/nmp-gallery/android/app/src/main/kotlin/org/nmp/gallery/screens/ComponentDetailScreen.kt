package org.nmp.gallery.screens

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import org.nmp.gallery.bridge.GalleryModel
import org.nmp.gallery.gallery.ContentComponentPage
import org.nmp.gallery.gallery.EmbedComponentPage
import org.nmp.gallery.gallery.RegistryComponent
import org.nmp.gallery.gallery.RegistrySection
import org.nmp.gallery.gallery.RelayComponentPage
import org.nmp.gallery.gallery.UserComponentPage

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ComponentDetailScreen(
    model: GalleryModel,
    section: RegistrySection,
    component: RegistryComponent,
    onBack: () -> Unit,
) {
    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(component.label) },
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(
                            imageVector = Icons.AutoMirrored.Filled.ArrowBack,
                            contentDescription = "Back",
                        )
                    }
                },
            )
        },
    ) { inner ->
        Box(
            modifier = Modifier
                .fillMaxSize()
                .padding(inner)
                .verticalScroll(rememberScrollState()),
        ) {
            when (section.id) {
                "relay" -> RelayComponentSection(model = model, component = component)
                "user" -> UserComponentPage(model = model, componentId = component.id)
                "content" -> ContentComponentPage(model = model, componentId = component.id)
                "embeds" -> EmbedsComponentSection(model = model, component = component)
                else -> Text("Unknown section: ${section.id}")
            }
        }
    }
}

@Composable
private fun RelayComponentSection(model: GalleryModel, component: RegistryComponent) {
    RelayComponentPage(model = model, componentId = component.id)
}

@Composable
private fun EmbedsComponentSection(model: GalleryModel, component: RegistryComponent) {
    EmbedComponentPage(model = model, componentId = component.id)
}
