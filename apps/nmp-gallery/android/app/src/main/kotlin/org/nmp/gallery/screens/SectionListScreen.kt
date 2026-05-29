package org.nmp.gallery.screens

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import org.nmp.gallery.gallery.RegistrySection

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SectionListScreen(
    sections: List<RegistrySection>,
    onSectionTap: (RegistrySection) -> Unit,
) {
    Scaffold(
        topBar = { TopAppBar(title = { Text("NMP Component Gallery") }) },
    ) { inner ->
        LazyColumn(
            modifier = Modifier
                .fillMaxWidth()
                .padding(inner),
            contentPadding = PaddingValues(vertical = 8.dp),
        ) {
            items(sections, key = { it.id }) { section ->
                SectionRow(section = section, onTap = { onSectionTap(section) })
                HorizontalDivider()
            }
        }
    }
}

@Composable
private fun SectionRow(section: RegistrySection, onTap: () -> Unit) {
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .clickable(onClick = onTap)
            .padding(horizontal = 16.dp, vertical = 12.dp),
    ) {
        Text(
            text = section.label,
            style = MaterialTheme.typography.titleMedium,
        )
        Text(
            text = "${section.components.size} components",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}
