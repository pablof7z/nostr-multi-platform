package org.nmp.android.ui

import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
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
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.FloatingActionButton
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextField
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.compositionLocalOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import org.nmp.android.KernelModel
import org.nmp.android.components.LocalNostrProfileHost
import org.nmp.android.components.NostrAvatar
import org.nmp.android.ui.embed.EventClaimer
import org.nmp.android.ui.embed.LocalRefEventEnvelopes
import org.nmp.android.ui.embed.LocalEventClaimer
import org.nmp.android.model.ChirpEventCard
import org.nmp.android.model.ChirpRootCard
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
        model.openHomeFeed()
    }
    val s by model.state.collectAsStateWithLifecycle()
    val snapshotCount by model.snapshotCount.collectAsStateWithLifecycle()
    val activeAccount = s.projections
        ?.accounts
        ?.firstOrNull { it.id == s.activeAccount }

    // V-85 OP-centric render: typed root cards from the NOFS decoder are the
    // sole home-feed source. The legacy `s.items` fallback (ADR-0037
    // Commitment 4) was removed in #920 — the kernel's `"timeline"` projection
    // was deleted in #924, so `s.items` is permanently empty.
    val opCards = s.modularTimeline.cards
    val hasOpFeed = opCards.isNotEmpty()

    var showComposeDialog by remember { mutableStateOf(false) }
    var selectedProfilePubkey by remember { mutableStateOf<String?>(null) }
    var selectedThreadId by remember { mutableStateOf<String?>(null) }

    val selectedProfile = selectedProfilePubkey
    if (selectedProfile != null) {
        ProfileScreen(
            pubkey = selectedProfile,
            model = model,
            onBack = { selectedProfilePubkey = null },
            modifier = modifier,
        )
        return
    }

    val selectedThread = selectedThreadId
    if (selectedThread != null) {
        ThreadScreen(
            eventId = selectedThread,
            model = model,
            onBack = { selectedThreadId = null },
            modifier = modifier,
        )
        return
    }

    // ADR-0063 Lane G (#1671): feed/list profile fetches resolve the small
    // ProfileRef shape with CacheOk liveness via the unified resolve_ref seam.
    val claimer: ProfileClaimer = { pubkey, consumerId, claim ->
        if (claim) model.resolveProfile(pubkey, consumerId)
        else model.releaseProfile(pubkey, consumerId)
    }
    val eventClaimer: EventClaimer = { uri, consumerId, claim ->
        if (claim) model.claimEvent(uri, consumerId)
        else model.releaseEvent(uri, consumerId)
    }

    val refEventEnvelopes = s.projections?.refEventEnvelopes ?: emptyMap()
    val profileHost = rememberKernelProfileHost(model)

    CompositionLocalProvider(
        LocalProfileClaimer provides claimer,
        LocalEventClaimer provides eventClaimer,
        LocalNostrProfileHost provides profileHost,
        LocalRefEventEnvelopes provides refEventEnvelopes,
    ) {
        Box(modifier.fillMaxSize()) {
            Column(Modifier.fillMaxSize()) {
                Row(
                    Modifier.fillMaxWidth().padding(16.dp),
                    horizontalArrangement = Arrangement.SpaceBetween,
                ) {
                    Text("Chirp", style = MaterialTheme.typography.headlineSmall)
                    Text(
                        "rev ${s.rev} · ${opCards.size} cards",
                        style = MaterialTheme.typography.labelSmall,
                    )
                }
                HorizontalDivider()
                if (!hasOpFeed) {
                    Placeholder(
                        // aim.md §2 / #979: the kernel ships the full `npub`; the
                        // Compose layer abbreviates for display via `shortHex`,
                        // exactly as iOS does (`account.npub.shortHex`, PR #1064).
                        activeAccountLabel = activeAccount?.npub?.let { shortHex(it) }
                            ?: s.activeAccount,
                        hasAccount = s.activeAccount.isNotEmpty(),
                        hasSnapshot = snapshotCount > 0,
                        lastErrorToast = s.localizedErrorToast,
                        onCreateAccount = { model.createLocalAccount() },
                    )
                } else {
                    // Typed OP-centric feed: one row per ChirpRootCard.
                    val page = s.modularTimeline.page
                    LazyColumn(Modifier.fillMaxSize()) {
                        itemsIndexed(opCards, key = { _, root -> root.card.id }) { index, root ->
                            val cursor = page?.nextCursor
                            if (index == opCards.lastIndex && page?.hasMore == true && cursor != null) {
                                LaunchedEffect(cursor) {
                                    model.loadOlderTimeline(cursor)
                                }
                            }
                            RootCardRow(
                                root = root,
                                items = emptyMap(),
                                model = model,
                                onAuthorClick = { selectedProfilePubkey = it },
                                onThreadClick = { selectedThreadId = it },
                            )
                            HorizontalDivider()
                        }
                    }
                }
            }
            FloatingActionButton(
                onClick = { showComposeDialog = true },
                modifier = Modifier
                    .align(Alignment.BottomEnd)
                    .padding(16.dp),
            ) {
                Icon(Icons.Filled.Add, contentDescription = "New note")
            }
        }
    }

    if (showComposeDialog) {
        ComposeNoteDialog(
            onDismiss = { showComposeDialog = false },
            onPublish = { content ->
                model.publishNote(content)
                showComposeDialog = false
            }
        )
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

/**
 * One row in the OP-centric feed: the root note plus an optional attribution
 * badge listing the follow(s) who referenced this root. Raw data only — no
 * display helpers inline (D8); the relative-time calculation below is a
 * presentation-layer concern acceptable here.
 */
@Composable
private fun RootCardRow(
    root: ChirpRootCard,
    items: Map<String, TimelineItem>,
    model: KernelModel,
    onAuthorClick: (String) -> Unit,
    onThreadClick: (String) -> Unit,
) {
    Column(
        Modifier
            .fillMaxWidth()
            .clickable { onThreadClick(root.card.id) }
    ) {
        NoteRow(
            eventId = root.card.id,
            items = items,
            cards = mapOf(root.card.id to root.card),
            model = model,
            onAuthorClick = onAuthorClick,
        )
        if (root.attribution.isNotEmpty()) {
            val label = root.attribution
                .take(3)
                .joinToString { it.authorDisplayName?.ifEmpty { it.authorPubkey.take(8) } ?: it.authorPubkey.take(8) }
            Text(
                "Replied by $label",
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(start = 56.dp, bottom = 4.dp),
            )
        }
    }
}

@Composable
@OptIn(ExperimentalFoundationApi::class)
internal fun NoteRow(
    eventId: String,
    items: Map<String, TimelineItem>,
    cards: Map<String, ChirpEventCard>,
    embedDepth: Int = 0,
    embedded: Boolean = false,
    model: KernelModel? = null,
    onAuthorClick: ((String) -> Unit)? = null,
) {
    val item = items[eventId]
    val card = cards[eventId]
    val content = item?.contentPreview?.ifEmpty { item.content }
        ?: card?.contentPreview?.ifEmpty { card.content }
    if (content == null) {
        MissingEventRow(eventId)
        return
    }
    val authorPubkey = card?.authorPubkey?.nonEmptyOrNull()
        ?: item?.authorPubkey?.nonEmptyOrNull()
        ?: ""
    // The NostrAvatar component below is self-claiming (claims the author kind:0
    // from the kernel via LocalNostrProfileHost on composition, releases on
    // dispose), so no manual RememberProfileClaim for the avatar is needed here.
    val shortPubkey = if (authorPubkey.length >= 16) {
        "${authorPubkey.take(8)}…${authorPubkey.takeLast(8)}"
    } else {
        authorPubkey.ifEmpty { "unknown" }
    }
    // Resolve author name: prefer the typed-feed card authorDisplayName, then
    // the per-key `refs.profile` keyed-ref cache (ADR-0063 Lane G — read via the
    // profile host, re-rendered per-key when this pubkey's kind:0 lands), then
    // shortPubkey. NostrAvatar already resolves+observes this pubkey, so the host
    // read below recomposes when the row commits.
    val profileHost = LocalNostrProfileHost.current
    val author = card?.authorDisplayName?.nonEmptyOrNull()
        ?: profileHost?.profileForPubkey(authorPubkey)?.displayName?.nonEmptyOrNull()
        ?: shortPubkey
    val createdAt = item?.createdAt?.takeIf { it > 0 }
        ?: card?.createdAt?.takeIf { it > 0 }
    val subtitle = createdAt?.let { ts ->
        val deltaSecs = (System.currentTimeMillis() / 1000) - ts
        when {
            deltaSecs < 60 -> "${deltaSecs}s ago"
            deltaSecs < 3_600 -> "${deltaSecs / 60}m ago"
            deltaSecs < 86_400 -> "${deltaSecs / 3_600}h ago"
            else -> "${deltaSecs / 86_400}d ago"
        }
    } ?: card?.let { "kind ${it.kind}" } ?: ""
    val relayProvenance = card?.relayProvenance?.takeIf { it.isNotEmpty() }
        ?: item?.relayProvenance
        ?: emptyList()
    val relayCount = if (relayProvenance.isNotEmpty()) relayProvenance.size.toLong() else item?.relayCount ?: 0
    var showRelayProvenance by remember { mutableStateOf(false) }
    val clipboard = LocalClipboardManager.current

    val rowPadding = if (embedded) 10.dp else 12.dp
    Column(Modifier.fillMaxWidth().padding(rowPadding)) {
        Row(
            verticalAlignment = Alignment.CenterVertically,
            modifier = Modifier.clickable(enabled = model != null && authorPubkey.isNotEmpty()) {
                if (authorPubkey.isNotEmpty()) {
                    onAuthorClick?.invoke(authorPubkey) ?: model?.openAuthor(authorPubkey)
                }
            }
        ) {
            NostrAvatar(
                pubkey = authorPubkey,
                size = 36.dp,
                consumerId = "note-author-$eventId",
            )
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
        val repost = card?.repostedBy
        if (repost != null) {
            val repostAuthor = repost.authorDisplayName?.nonEmptyOrNull()
                ?: repost.authorPubkey.take(8)
            Text(
                "Repost by $repostAuthor",
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Spacer(Modifier.size(4.dp))
        }
        NostrRichText(
            content = content,
            contentTree = card?.contentTree,
            items = items,
            cards = cards,
            embedDepth = embedDepth,
        )
        if (relayCount > 0) {
            Spacer(Modifier.size(6.dp))
            Text(
                "Received from $relayCount ${if (relayCount == 1L) "relay" else "relays"}",
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.combinedClickable(
                    onClick = { showRelayProvenance = true },
                    onLongClick = { showRelayProvenance = true },
                ),
            )
        }
        Spacer(Modifier.size(8.dp))
        NoteActionsSummary(card, model)
    }
    if (showRelayProvenance) {
        AlertDialog(
            onDismissRequest = { showRelayProvenance = false },
            title = { Text("Received from") },
            text = {
                Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    if (relayProvenance.isEmpty()) {
                        Text("No relay provenance", color = MaterialTheme.colorScheme.onSurfaceVariant)
                    } else {
                        relayProvenance.forEach { relay ->
                            Text(
                                relay,
                                style = MaterialTheme.typography.bodySmall,
                                modifier = Modifier
                                    .fillMaxWidth()
                                    .clickable { clipboard.setText(AnnotatedString(relay)) },
                            )
                        }
                    }
                }
            },
            confirmButton = {
                Button(onClick = { showRelayProvenance = false }) {
                    Text("Done")
                }
            },
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

private fun String.nonEmptyOrNull(): String? = if (isEmpty()) null else this
