package org.nmp.android.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.compositionLocalOf
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import org.nmp.android.KernelModel
import org.nmp.android.model.ChirpEventCard
import org.nmp.android.model.ModuleTimelineBlock
import org.nmp.android.model.StandaloneTimelineBlock
import org.nmp.android.model.TimelineBlock
import org.nmp.android.model.TimelineItem

/**
 * Per-view callbacks for demand-driven profile fetching. The presentation
 * layer claims a pubkey when it begins rendering and releases on
 * `DisposableEffect.onDispose`. The kernel batches the kind:0 REQ and
 * re-fetches against the author's NIP-65 write set once it lands.
 *
 * `LocalProfileClaimer.current` is `null` outside a provider scope; the
 * `RememberProfileClaim` composable below treats that as a no-op so the
 * call sites stay non-conditional.
 */
typealias ProfileClaimer = (pubkey: String, consumerId: String, claim: Boolean) -> Unit

val LocalProfileClaimer = compositionLocalOf<ProfileClaimer?> { null }

/**
 * Lightweight 64-hex pubkey gate. Mirrors the C-ABI `is_hex_pubkey` guard so
 * the JNI shim's silent no-op never fires from an obviously-wrong key (avoids
 * pointless JNI round-trips). Decoders that hand us short/empty pubkeys
 * (cold-start, missing data) are filtered here.
 */
private fun isHexPubkey64(value: String): Boolean {
    if (value.length != 64) return false
    return value.all { it.isDigit() || it in 'a'..'f' || it in 'A'..'F' }
}

/**
 * Claim [pubkey] on enter, release on dispose. No-op when:
 *  - `LocalProfileClaimer.current` is null (outside a provider scope), or
 *  - [pubkey] is not a 64-char hex string.
 *
 * Stable [consumerId] (caller-supplied) so a recompose with the same [pubkey]
 * does not churn the kernel's per-pubkey claim slot.
 */
@Composable
fun RememberProfileClaim(pubkey: String, consumerId: String) {
    val claimer = LocalProfileClaimer.current ?: return
    if (!isHexPubkey64(pubkey)) return
    DisposableEffect(pubkey, consumerId) {
        claimer(pubkey, consumerId, true)
        onDispose { claimer(pubkey, consumerId, false) }
    }
}

/**
 * Live kind:1 feed straight from the kernel snapshot — Android peer of iOS
 * `TimelineView`. Renders verbatim; no sorting/derivation (D8).
 */
@Composable
fun TimelineScreen(model: KernelModel, modifier: Modifier = Modifier) {
    LaunchedEffect(model) {
        model.openTimeline()
    }
    val s by model.state.collectAsStateWithLifecycle()
    val snapshotCount by model.snapshotCount.collectAsStateWithLifecycle()
    val activeAccount = s.projections
        ?.accounts
        ?.firstOrNull { it.id == s.activeAccount }
    val itemLookup = s.items.associateBy { it.id }
    val cardLookup = s.modularTimeline.cards.associateBy { it.id }
    val blocks = if (s.modularTimeline.blocks.isNotEmpty()) {
        s.modularTimeline.blocks
    } else {
        s.items.map { StandaloneTimelineBlock(it.id) }
    }

    val claimer: ProfileClaimer = { pubkey, consumerId, claim ->
        if (claim) model.claimProfile(pubkey, consumerId)
        else model.releaseProfile(pubkey, consumerId)
    }

    CompositionLocalProvider(LocalProfileClaimer provides claimer) {
        Column(modifier.fillMaxSize()) {
            Row(
                Modifier.fillMaxWidth().padding(16.dp),
                horizontalArrangement = Arrangement.SpaceBetween,
            ) {
                Text("Chirp", style = MaterialTheme.typography.headlineSmall)
                Text(
                    "rev ${s.rev} · ${blocks.size} blocks",
                    style = MaterialTheme.typography.labelSmall,
                )
            }
            HorizontalDivider()
            if (blocks.isEmpty()) {
                Placeholder(
                    activeAccountLabel = activeAccount?.npubShort ?: s.activeAccount,
                    hasAccount = s.activeAccount.isNotEmpty(),
                    hasSnapshot = snapshotCount > 0,
                    lastErrorToast = s.lastErrorToast,
                    onCreateAccount = { model.createLocalAccount() },
                )
            } else {
                LazyColumn(Modifier.fillMaxSize()) {
                    itemsIndexed(blocks, key = { index, block -> blockKey(index, block) }) { _, block ->
                        TimelineBlockRow(block, itemLookup, cardLookup)
                        HorizontalDivider()
                    }
                }
            }
        }
    }
}

@Composable
private fun Placeholder(
    activeAccountLabel: String,
    hasAccount: Boolean,
    hasSnapshot: Boolean,
    lastErrorToast: String?,
    onCreateAccount: () -> Unit,
) {
    val message = if (hasAccount) {
        "No timeline events yet"
    } else {
        lastErrorToast?.nonEmptyOrNull() ?: if (hasSnapshot) "No active account" else "Starting kernel…"
    }
    Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
        Column(horizontalAlignment = Alignment.CenterHorizontally) {
            if (!hasSnapshot) {
                CircularProgressIndicator()
                Spacer(Modifier.size(16.dp))
            }
            Text(
                message,
                textAlign = TextAlign.Center,
                modifier = Modifier.padding(horizontal = 24.dp),
            )
            if (hasAccount) {
                Spacer(Modifier.size(8.dp))
                Text(
                    "Active account: $activeAccountLabel",
                    style = MaterialTheme.typography.bodySmall,
                    textAlign = TextAlign.Center,
                    modifier = Modifier.padding(horizontal = 24.dp),
                )
            } else if (hasSnapshot) {
                Spacer(Modifier.size(16.dp))
                Button(onClick = onCreateAccount) {
                    Text("Create local account")
                }
            }
        }
    }
}

@Composable
private fun TimelineBlockRow(
    block: TimelineBlock,
    items: Map<String, TimelineItem>,
    cards: Map<String, ChirpEventCard>,
) {
    when (block) {
        is StandaloneTimelineBlock -> NoteRow(block.eventId, items, cards)
        is ModuleTimelineBlock -> ModuleBlockRow(block, items, cards)
    }
}

@Composable
private fun ModuleBlockRow(
    block: ModuleTimelineBlock,
    items: Map<String, TimelineItem>,
    cards: Map<String, ChirpEventCard>,
) {
    Column(Modifier.fillMaxWidth().padding(vertical = 4.dp)) {
        block.events.forEachIndexed { index, eventId ->
            NoteRow(eventId, items, cards)
            if (index < block.events.lastIndex) {
                HorizontalDivider(Modifier.padding(start = 56.dp))
            }
        }
        if (block.hasGap) {
            Text(
                "Thread has more context",
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(start = 56.dp, bottom = 8.dp),
            )
        }
    }
}

@Composable
internal fun NoteRow(
    eventId: String,
    items: Map<String, TimelineItem>,
    cards: Map<String, ChirpEventCard>,
    embedDepth: Int = 0,
    embedded: Boolean = false,
) {
    val item = items[eventId]
    val card = cards[eventId]
    val content = item?.contentPreview?.ifEmpty { item.content }
        ?: card?.contentPreview?.ifEmpty { card.content }
    if (content == null) {
        MissingEventRow(eventId)
        return
    }
    // Demand-driven kind:0 fetch for the note author. Stable consumer-id
    // (`note-author-<eventId>`) so two re-renders of the same row do not
    // churn the kernel's per-pubkey claim slot. The kernel's `claim_profile`
    // is idempotent on `(pubkey, consumer_id)`.
    //
    // Only `ChirpEventCard` carries the raw author pubkey in the snapshot;
    // `TimelineItem` ships the displayable name only (snapshot.rs design).
    // No card → no claim (cold-start, missing card metadata).
    val authorPubkey = card?.authorPubkey.orEmpty()
    if (authorPubkey.isNotEmpty()) {
        RememberProfileClaim(authorPubkey, "note-author-$eventId")
    }
    val author = item?.authorDisplay?.nonEmptyOrNull()
        ?: card?.authorDisplayName?.nonEmptyOrNull()
        ?: card?.authorPubkeyShort?.nonEmptyOrNull()
        ?: "unknown"
    val initials = item?.authorAvatarInitials?.nonEmptyOrNull()
        ?: card?.authorAvatarInitials?.nonEmptyOrNull()
        ?: author.take(2).uppercase()
    val color = item?.authorAvatarColor?.nonEmptyOrNull()
        ?: card?.authorAvatarColor.orEmpty()
    val subtitle = item?.createdAtDisplay?.nonEmptyOrNull()
        ?: card?.createdAtDisplay?.nonEmptyOrNull()
        ?: card?.let { "kind ${it.kind}" }
        ?: ""

    val rowPadding = if (embedded) 10.dp else 12.dp
    Column(Modifier.fillMaxWidth().padding(rowPadding)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Avatar(initials, color)
            Spacer(Modifier.size(8.dp))
            Column {
                Text(
                    author,
                    style = MaterialTheme.typography.titleSmall,
                    fontWeight = FontWeight.Bold,
                )
                Text(
                    subtitle,
                    style = MaterialTheme.typography.labelSmall,
                )
            }
        }
        Spacer(Modifier.size(6.dp))
        NostrRichText(
            content = content,
            contentTree = card?.contentTree,
            items = items,
            cards = cards,
            embedDepth = embedDepth,
        )
    }
}

@Composable
private fun MissingEventRow(eventId: String) {
    Text(
        "Event pending ${eventId.take(8)}",
        style = MaterialTheme.typography.bodySmall,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        modifier = Modifier.fillMaxWidth().padding(12.dp),
    )
}

@Composable
private fun Avatar(initials: String, colorHex: String) {
    Surface(
        modifier = Modifier.size(36.dp).clip(CircleShape),
        color = parseHexColor(colorHex) ?: MaterialTheme.colorScheme.secondary,
    ) {
        Box(contentAlignment = Alignment.Center) {
            Text(
                initials,
                color = Color.White,
                style = MaterialTheme.typography.labelMedium,
                fontWeight = FontWeight.Bold,
            )
        }
    }
}

/** `#RRGGBB` / `RRGGBB` → Color; null on malformed (caller falls back). */
private fun parseHexColor(hex: String): Color? {
    val clean = hex.trim().removePrefix("#")
    if (clean.length != 6) return null
    val v = clean.toLongOrNull(16) ?: return null
    return Color(
        red = ((v shr 16) and 0xFF) / 255f,
        green = ((v shr 8) and 0xFF) / 255f,
        blue = (v and 0xFF) / 255f,
    )
}

private fun blockKey(index: Int, block: TimelineBlock): String {
    val ids = block.eventIds.joinToString(":")
    return ids.ifEmpty { "block-$index" }
}

private fun String.nonEmptyOrNull(): String? = if (isEmpty()) null else this
