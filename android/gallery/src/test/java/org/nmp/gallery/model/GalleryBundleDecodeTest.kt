package org.nmp.gallery.model

import java.io.File
import kotlinx.serialization.json.Json
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test

class GalleryBundleDecodeTest {
    private val json = Json {
        ignoreUnknownKeys = true
        isLenient = true
        classDiscriminator = "type"
    }

    @Test
    fun contentGalleryBundleDecodesContentTreeWire() {
        val bundleFile = File("src/main/assets/content-gallery-bundle.json")
        val bundle = json.decodeFromString(GalleryBundle.serializer(), bundleFile.readText())

        assertEquals(2, bundle.version)
        assertTrue(bundle.scenarios.isNotEmpty())
        assertTrue(bundle.scenarios.all { it.rendered.nodes.isNotEmpty() || it.rendered.roots.isEmpty() })
        assertTrue(bundle.scenarios.any { scenario ->
            scenario.rendered.nodes.any { it is WireNode.EventRef }
        })

        val embeddedTree = bundle.scenarios
            .flatMap { it.embeds.values }
            .firstNotNullOfOrNull { it.rendered }
        assertNotNull(embeddedTree)
        assertTrue(embeddedTree!!.nodes.isNotEmpty() || embeddedTree.roots.isEmpty())
    }

    @Test
    fun contentGalleryBundleCarriesOpaqueCycleKeys() {
        val bundleFile = File("src/main/assets/content-gallery-bundle.json")
        val bundle = json.decodeFromString(GalleryBundle.serializer(), bundleFile.readText())

        val entries = bundle.scenarios.flatMap { it.embeds.values }
        assertTrue(entries.isNotEmpty())
        assertTrue(entries.all { it.cycleKey.isNotBlank() })

        val cycle = bundle.scenarios.first { it.id == "S-M09" }
        val cycleKeys = cycle.embeds.values.map { it.cycleKey }.toSet()
        assertEquals(2, cycleKeys.size)
        assertTrue(cycleKeys.all { it.startsWith("30023:") })
    }
}
