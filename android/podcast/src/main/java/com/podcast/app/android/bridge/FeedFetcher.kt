package com.podcast.app.android.bridge

import android.util.Log
import okhttp3.OkHttpClient
import okhttp3.Request
import java.io.IOException
import java.util.concurrent.TimeUnit

private const val TAG = "FeedFetcher"

/**
 * Host-side HTTP transport for RSS/Atom feed bytes — T-podcast-android-4.
 *
 * Implements Option A from T-podcast-gap-3: the Android host owns the network
 * fetch; Rust owns the parse. This class fetches raw bytes from a feed URL
 * and hands them to [PodcastKernelBridge.ingestBytes] so the Rust parser can
 * populate the episode list.
 *
 * Doctrine:
 *   - D5: no business logic. No feed parsing here.
 *   - D6: all network failures are surfaced as [FetchResult.Failure] with a
 *     human-readable reason; never silently swallowed or faked.
 *   - D0: no podcast nouns in nmp-core; fetch lives entirely in the host shell.
 *
 * The [OkHttpClient] is shared across fetches (connection pooling). Timeouts
 * are conservative for RSS feeds (~5 MiB max in practice).
 */
class FeedFetcher {

    private val client = OkHttpClient.Builder()
        .connectTimeout(15, TimeUnit.SECONDS)
        .readTimeout(30, TimeUnit.SECONDS)
        .followRedirects(true)
        .build()

    sealed class FetchResult {
        /** Feed bytes were retrieved and accepted by the Rust parser. */
        data class Success(val episodeCount: Int) : FetchResult()

        /** Network or parse failure with a user-surfaceable reason (D6). */
        data class Failure(val reason: String) : FetchResult()
    }

    /**
     * Fetch bytes from [feedUrl] and ingest them via [bridge].
     *
     * Must be called from a background coroutine (blocking I/O).
     * Returns a [FetchResult] the caller can surface as a toast (D6).
     */
    fun fetchAndIngest(bridge: PodcastKernelBridge, feedUrl: String): FetchResult {
        Log.i(TAG, "fetchAndIngest: $feedUrl")

        val bytes = try {
            val request = Request.Builder()
                .url(feedUrl)
                .header("User-Agent", "NmpPodcast/1.0 (Android; feed-fetch)")
                .build()
            client.newCall(request).execute().use { response ->
                if (!response.isSuccessful) {
                    val reason = "HTTP ${response.code} from feed"
                    Log.w(TAG, reason)
                    return FetchResult.Failure(reason)
                }
                val body = response.body
                if (body == null) {
                    Log.w(TAG, "empty response body for: $feedUrl")
                    return FetchResult.Failure("Empty response from feed server")
                }
                body.bytes()
            }
        } catch (e: IllegalArgumentException) {
            // OkHttp throws this on a malformed URL (e.g. invalid scheme).
            val reason = "Invalid feed URL: ${e.message}"
            Log.w(TAG, reason)
            return FetchResult.Failure(reason)
        } catch (e: IOException) {
            val reason = "Network error: ${e.message}"
            Log.w(TAG, reason)
            return FetchResult.Failure(reason)
        }

        val statusJson = bridge.ingestBytes(feedUrl, bytes)
            ?: return FetchResult.Failure("Podcast handle not ready")

        // Parse `{"ok":true,"episode_count":N}` or `{"ok":false,"reason":"..."}`.
        return parseIngestStatus(statusJson)
    }

    private fun parseIngestStatus(json: String): FetchResult {
        // Lightweight manual parse — avoids pulling kotlinx.serialization for
        // a two-field JSON blob and keeps the bridge layer dependency-clean.
        return try {
            val ok = Regex("\"ok\"\\s*:\\s*(true|false)").find(json)
                ?.groupValues?.get(1) == "true"
            if (ok) {
                val count = Regex("\"episode_count\"\\s*:\\s*(\\d+)").find(json)
                    ?.groupValues?.get(1)?.toIntOrNull() ?: 0
                FetchResult.Success(count)
            } else {
                val reason = Regex("\"reason\"\\s*:\\s*\"([^\"]+)\"").find(json)
                    ?.groupValues?.get(1) ?: "Feed parse failed"
                FetchResult.Failure(reason)
            }
        } catch (e: Exception) {
            Log.e(TAG, "failed to parse ingest status: $json — ${e.message}")
            FetchResult.Failure("Unexpected response from feed parser")
        }
    }
}
