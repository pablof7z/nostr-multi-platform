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
    }

    @Test
    fun contentGalleryBundleDecodesContentTreeWire() {
        val bundleFile = File("src/main/assets/content-gallery-bundle.json")
        val bundle = json.decodeFromString(GalleryBundle.serializer(), bundleFile.readText())

        assertEquals(3, bundle.version)
        assertTrue(bundle.scenarios.isNotEmpty())
        assertTrue(bundle.scenarios.all { it.rendered.nodes.isNotEmpty() || it.rendered.roots.isEmpty() })
        assertTrue(bundle.scenarios.any { scenario ->
            scenario.rendered.nodes.any { it is WireNode.EventRef }
        })
    }

    @Test
    fun contentGalleryBundleCarriesKindRegistryProjections() {
        val bundleFile = File("src/main/assets/content-gallery-bundle.json")
        val bundle = json.decodeFromString(GalleryBundle.serializer(), bundleFile.readText())

        val envelopes = bundle.scenarios.flatMap { it.embeds.values }
        assertTrue(envelopes.isNotEmpty())

        // Every non-collapsed envelope must carry a typed projection.
        val resolved = envelopes.filter { !it.collapsed }
        assertTrue(resolved.isNotEmpty())
        assertTrue(resolved.all { it.projection != null })

        // At least one short-note projection must be present (S-M04 / S-M05 / …).
        val shortNotes = resolved.count { it.projection is GalleryEmbedKindProjection.ShortNote }
        assertTrue("expected at least one ShortNote projection", shortNotes > 0)

        // Profile projections must be present (S-M01 / S-M02 / …).
        val profiles = resolved.count { it.projection is GalleryEmbedKindProjection.Profile }
        assertTrue("expected at least one Profile projection", profiles > 0)

        // Article projections (kind:30023) from S-M09.
        val articles = resolved.count { it.projection is GalleryEmbedKindProjection.Article }
        assertTrue("expected at least one Article projection", articles > 0)
    }

    @Test
    fun contentGalleryBundleArticleProjectionHasTitleInSM09() {
        val bundleFile = File("src/main/assets/content-gallery-bundle.json")
        val bundle = json.decodeFromString(GalleryBundle.serializer(), bundleFile.readText())

        val s09 = bundle.scenarios.firstOrNull { it.id == "S-M09" }
        assertNotNull("S-M09 scenario must exist", s09)

        val articleEnvelopes = s09!!.embeds.values.filter {
            it.projection is GalleryEmbedKindProjection.Article
        }
        assertTrue("S-M09 must have article embeds", articleEnvelopes.isNotEmpty())
        val article = (articleEnvelopes.first().projection as GalleryEmbedKindProjection.Article).data
        assertNotNull("article must have a non-null title", article.title)
    }

    @Test
    fun contentGalleryBundleCollapsedEmbedsHaveNoProjection() {
        val bundleFile = File("src/main/assets/content-gallery-bundle.json")
        val bundle = json.decodeFromString(GalleryBundle.serializer(), bundleFile.readText())

        val collapsed = bundle.scenarios.flatMap { it.embeds.values }.filter { it.collapsed }
        assertTrue("bundle must contain collapsed embed stubs", collapsed.isNotEmpty())
        // Collapsed entries must not carry a projection.
        assertTrue(collapsed.all { it.projection == null })
        // All collapsed entries must carry a reason.
        assertTrue(collapsed.all { it.collapseReason != null })
    }
}
