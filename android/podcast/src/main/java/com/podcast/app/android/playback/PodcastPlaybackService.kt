package com.podcast.app.android.playback

import androidx.media3.common.AudioAttributes
import androidx.media3.common.C
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.session.MediaSession
import androidx.media3.session.MediaSessionService

/**
 * Minimal foreground [MediaSessionService] that hosts an [ExoPlayer] instance.
 *
 * T-podcast-android-7: MVP scope — keeps playback alive when the screen is off
 * or the user navigates away. The session is exposed to the system media
 * controls via [MediaSession].
 *
 * ExoPlayer is configured with [AudioAttributes] that target USAGE_MEDIA /
 * CONTENT_TYPE_SPEECH (podcast content), with [handleAudioFocus] = true so
 * the OS manages audio focus automatically.
 *
 * Doctrine compliance:
 *   - D5: zero business logic — the service only manages the player lifecycle.
 *   - D6: playback state is driven by ExoPlayer (honest OS state).
 *   - No podcast/feed/RSS nouns in crates/nmp-core (D0) — this is host-side.
 */
class PodcastPlaybackService : MediaSessionService() {

    private var player: ExoPlayer? = null
    private var session: MediaSession? = null

    override fun onCreate() {
        super.onCreate()
        val audioAttributes = AudioAttributes.Builder()
            .setUsage(C.USAGE_MEDIA)
            .setContentType(C.AUDIO_CONTENT_TYPE_SPEECH)
            .build()

        val exoPlayer = ExoPlayer.Builder(this)
            .setAudioAttributes(audioAttributes, /* handleAudioFocus= */ true)
            .build()

        player = exoPlayer
        session = MediaSession.Builder(this, exoPlayer).build()
    }

    override fun onGetSession(controllerInfo: MediaSession.ControllerInfo): MediaSession? =
        session

    override fun onDestroy() {
        session?.run {
            player.release()
            release()
        }
        session = null
        player = null
        super.onDestroy()
    }
}
