package org.nmp.android.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TextField
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import org.nmp.android.KernelModel
import org.nmp.android.model.MarmotGroup
import org.nmp.android.model.MarmotKeyPackage
import org.nmp.android.model.MarmotLastOpError
import org.nmp.android.model.MarmotPendingOp
import org.nmp.android.model.MarmotPendingWelcome
import org.nmp.android.model.MarmotSnapshot

/**
 * Row / banner / dialog composables for the Marmot group list — extracted from
 * `GroupsScreen.kt` so neither file crosses the 500-LOC hard cap (AGENTS.md).
 *
 * Thin-shell rule (aim.md §2): ZERO protocol logic. Presentation strings
 * (`displayLabel`, `displayName`, `initials`, `invitesChipLabel`) are computed
 * by the shell from raw wire data; `lastOpError` machine codes are mapped to
 * banner copy here too. The only structural chrome is section headers /
 * field labels and the key-package subtitle / action label.
 */
@Composable
internal fun KeyPackageRow(model: KernelModel, snapshot: MarmotSnapshot) {
    val kp = snapshot.keyPackage
    Row(
        Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 12.dp),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(Modifier.weight(1f)) {
            Text("Key package", style = MaterialTheme.typography.titleSmall, fontWeight = FontWeight.Bold)
            Text(
                keyPackageSubtitle(kp),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        Spacer(Modifier.size(8.dp))
        OutlinedButton(
            onClick = { model.marmot.publishKeyPackage() },
            enabled = kp.isRegistered,
        ) {
            Text(if (kp.published) "Rotate key package" else "Publish key package")
        }
    }
    HorizontalDivider()
}

internal fun bucketAge(secs: Long): String = when {
    secs < 60 -> "${secs}s old"
    secs < 3_600 -> "${secs / 60}m old"
    secs < 86_400 -> "${secs / 3_600}h old"
    else -> "${secs / 86_400}d old"
}

/// Shared shell-side key-package subtitle (aim.md §2 — presentation in the
/// shell). Used by both `KeyPackageRow` (registered) and `GroupsScreen`'s
/// not-registered empty state, so the copy lives in exactly one place.
internal fun keyPackageSubtitle(kp: MarmotKeyPackage): String {
    if (!kp.isRegistered) return "Sign in with an nsec to enable"
    if (!kp.published) return "Not published"
    val parts = mutableListOf("Published")
    kp.ageSecs?.let { parts.add(bucketAge(it)) }
    if (kp.stale) parts.add("needs rotation")
    return parts.joinToString(" · ")
}

@Composable
internal fun PendingOpRow(op: MarmotPendingOp) {
    Row(
        Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        CircularProgressIndicator(modifier = Modifier.size(16.dp), strokeWidth = 2.dp)
        Spacer(Modifier.size(8.dp))
        // Shell-computed display label (aim.md §2 — presentation, not wire data).
        Text(
            op.displayLabel,
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

@Composable
internal fun PendingWelcomeRow(model: KernelModel, welcome: MarmotPendingWelcome) {
    Row(
        Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(Modifier.weight(1f)) {
            Text(welcome.displayName, style = MaterialTheme.typography.titleSmall, maxLines = 1)
            Text(
                "from ${shortHex(welcome.inviterNpub)}",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        TextButton(onClick = { model.marmot.declineWelcome(welcome.idHex) }) { Text("Decline") }
        Button(onClick = { model.marmot.acceptWelcome(welcome.idHex) }) { Text("Accept") }
    }
}

@Composable
internal fun GroupRow(group: MarmotGroup, onClick: () -> Unit) {
    Row(
        Modifier
            .fillMaxWidth()
            .clickable(onClick = onClick)
            .padding(horizontal = 12.dp, vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Surface(
            modifier = Modifier.size(40.dp).clip(RoundedCornerShape(20.dp)),
            color = MaterialTheme.colorScheme.secondaryContainer,
        ) {
            Box(contentAlignment = Alignment.Center) {
                Text(group.initials, fontWeight = FontWeight.Bold)
            }
        }
        Spacer(Modifier.size(12.dp))
        Column(Modifier.weight(1f)) {
            Text(group.displayName, style = MaterialTheme.typography.titleSmall, fontWeight = FontWeight.Bold, maxLines = 1)
            Text(
                "${group.memberCount} members",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

/**
 * Create group dialog with proper async feedback.
 *
 * [isWaiting] is true from the moment the op is dispatched until a terminal
 * verdict arrives. [pendingOpRow] carries the Rust-owned display_label while
 * the op is parked in the deferred-completion store. [errorMessage] surfaces
 * terminal failures verbatim. Dismiss only happens on `"accepted"` (driven by
 * the parent `GroupListScreen` via `LaunchedEffect`).
 */
@Composable
internal fun CreateGroupDialog(
    isWaiting: Boolean,
    pendingOpRow: MarmotPendingOp?,
    errorMessage: String?,
    onDismiss: () -> Unit,
    onCreate: (String, String) -> Unit,
) {
    var name by remember { mutableStateOf("") }
    var invitees by remember { mutableStateOf("") }

    AlertDialog(
        onDismissRequest = { if (!isWaiting) onDismiss() },
        title = { Text("New group") },
        text = {
            Column {
                TextField(
                    value = name,
                    onValueChange = { if (!isWaiting) name = it },
                    label = { Text("Group name") },
                    singleLine = true,
                    enabled = !isWaiting,
                )
                Spacer(Modifier.size(8.dp))
                TextField(
                    value = invitees,
                    onValueChange = { if (!isWaiting) invitees = it },
                    label = { Text("Invite npubs (optional)") },
                    enabled = !isWaiting,
                )
                // Pending row: shell-computed displayLabel (aim.md §2).
                if (pendingOpRow != null) {
                    Spacer(Modifier.size(8.dp))
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        CircularProgressIndicator(modifier = Modifier.size(14.dp), strokeWidth = 2.dp)
                        Spacer(Modifier.size(6.dp))
                        Text(
                            pendingOpRow.displayLabel,
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                }
                // Spinner when dispatched but not yet in pendingOps.
                if (isWaiting && pendingOpRow == null) {
                    Spacer(Modifier.size(8.dp))
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        CircularProgressIndicator(modifier = Modifier.size(14.dp), strokeWidth = 2.dp)
                        Spacer(Modifier.size(6.dp))
                        Text("Creating…", style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant)
                    }
                }
                // Error: Rust-owned reason rendered verbatim.
                if (errorMessage != null) {
                    Spacer(Modifier.size(8.dp))
                    Text(
                        errorMessage,
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.error,
                    )
                }
            }
        },
        confirmButton = {
            Button(
                onClick = { onCreate(name.trim(), invitees.trim()) },
                enabled = name.trim().isNotEmpty() && !isWaiting,
            ) {
                if (isWaiting) {
                    CircularProgressIndicator(modifier = Modifier.size(16.dp), strokeWidth = 2.dp)
                } else {
                    Text("Create")
                }
            }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Cancel") } },
    )
}

@Composable
internal fun GroupSectionHeader(title: String) {
    Text(
        title,
        style = MaterialTheme.typography.labelLarge,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp),
    )
}

@Composable
internal fun EmptyGroupsHint() {
    Text(
        "No groups yet. Tap + to create an encrypted group.",
        style = MaterialTheme.typography.bodyMedium,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        modifier = Modifier.padding(16.dp),
    )
}

@Composable
internal fun WarningBanner(text: String) {
    Surface(color = MaterialTheme.colorScheme.errorContainer, modifier = Modifier.fillMaxWidth()) {
        Text(
            text,
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onErrorContainer,
            modifier = Modifier.padding(12.dp),
        )
    }
}

/**
 * Map a Rust-owned [MarmotLastOpError] machine code to a user-facing banner.
 *
 * Per aim.md §2 / the `LastOpError` schema doc, `reason` is raw data (a machine
 * code such as `"key_package_unavailable"`) and the SHELL maps it to a banner —
 * this is the one sanctioned place for native-side failure copy. Unknown codes
 * fall back to a generic message tagged with the op, so a new Rust reason never
 * renders a bare code or an empty banner (D6 resilience).
 */
internal fun marmotErrorBanner(err: MarmotLastOpError): String {
    val opLabel = when (err.op) {
        "create_group" -> "create the group"
        "invite" -> "send the invite"
        else -> "complete the last action"
    }
    return when (err.reason) {
        "key_package_unavailable" ->
            "Couldn't $opLabel — a member has no key package published yet. Try again later."
        else -> "Couldn't $opLabel."
    }
}

@Composable
internal fun NotRegisteredState(subtitle: String) {
    Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
        Column(horizontalAlignment = Alignment.CenterHorizontally) {
            Text("Groups unavailable", style = MaterialTheme.typography.headlineSmall)
            Spacer(Modifier.size(8.dp))
            Text(
                subtitle.ifEmpty { "Encrypted groups require a local signing key." },
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                textAlign = TextAlign.Center,
                modifier = Modifier.padding(horizontal = 24.dp),
            )
        }
    }
}

/**
 * Render-grade hex shortening (presentation only; never decides behaviour).
 * `internal` so the sibling `GroupChatView` in `GroupChatScreen.kt` reuses the
 * single canonical copy rather than duplicating it (no fragmentation).
 */
internal fun shortHex(hex: String): String =
    if (hex.length >= 16) "${hex.take(8)}…${hex.takeLast(8)}" else hex
