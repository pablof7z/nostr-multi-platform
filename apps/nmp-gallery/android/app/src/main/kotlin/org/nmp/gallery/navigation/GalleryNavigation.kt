package org.nmp.gallery.navigation

import androidx.compose.runtime.Composable
import androidx.navigation.NavType
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.rememberNavController
import androidx.navigation.navArgument
import org.nmp.gallery.bridge.GalleryModel
import org.nmp.gallery.gallery.findComponent
import org.nmp.gallery.gallery.findSection
import org.nmp.gallery.screens.ComponentDetailScreen
import org.nmp.gallery.screens.ComponentListScreen
import org.nmp.gallery.screens.SectionListScreen

/**
 * Single NavHost driving the gallery's three screens:
 *
 *   sections                       — top-level RegistrySection list
 *   components/{sectionId}         — components within a section
 *   detail/{sectionId}/{compId}    — live demo of a single component
 *
 * `sectionId` lives in the detail route alongside `componentId` so the
 * detail screen can route to the user / content rendering family without
 * a back-trip to find-by-id (handy if the user opens the app deep-linked).
 */
@Composable
fun GalleryNavigation(model: GalleryModel) {
    val nav = rememberNavController()
    NavHost(navController = nav, startDestination = "sections") {
        composable("sections") {
            SectionListScreen(onSectionTap = { section ->
                nav.navigate("components/${section.id}")
            })
        }
        composable(
            route = "components/{sectionId}",
            arguments = listOf(navArgument("sectionId") { type = NavType.StringType }),
        ) { entry ->
            val sectionId = entry.arguments?.getString("sectionId").orEmpty()
            val section = findSection(sectionId) ?: return@composable
            ComponentListScreen(
                section = section,
                onComponentTap = { component ->
                    nav.navigate("detail/${section.id}/${component.id}")
                },
                onBack = { nav.popBackStack() },
            )
        }
        composable(
            route = "detail/{sectionId}/{componentId}",
            arguments = listOf(
                navArgument("sectionId") { type = NavType.StringType },
                navArgument("componentId") { type = NavType.StringType },
            ),
        ) { entry ->
            val sectionId = entry.arguments?.getString("sectionId").orEmpty()
            val componentId = entry.arguments?.getString("componentId").orEmpty()
            val section = findSection(sectionId) ?: return@composable
            val (_, component) = findComponent(componentId) ?: return@composable
            ComponentDetailScreen(
                model = model,
                section = section,
                component = component,
                onBack = { nav.popBackStack() },
            )
        }
    }
}
