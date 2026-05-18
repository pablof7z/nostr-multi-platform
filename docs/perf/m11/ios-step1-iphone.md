# T156 Step 1 — Deployment evidence

Device: Pablo's iPhone (UDID 3C438D9B-2021-5A30-93DB-910F7754F9A2),
        iPhone 17 Pro Max (iPhone18,2), iOS available paired.
Build:  /tmp/NmpPodcast-DD-Device/Build/Products/Debug-iphoneos/NmpPodcast.app
Bundle: com.podcast.app
Install timestamp: 2026-05-18 19:24:21 (local)
Launch timestamp: 2026-05-18 19:24:29 (local)
Confirmed running: `devicectl device info processes` shows PID 16430 for
                   /private/var/containers/Bundle/Application/36C17837-57E0-4120-9243-D5FF735FB04F/NmpPodcast.app/NmpPodcast

User-facing scenario now live on Pablo's iPhone:
- Open NmpPodcast → Library tab shows "No Podcasts" ContentUnavailableView.
- Tap toolbar `+` → Add Podcast sheet appears (Feed URL field + optional title/author).
- Enter feed URL → tap Add → row appears in the kernel-backed Library list.
- Swipe-to-delete → row removed (kernel snapshot reflects unsubscribe).
- Force-quit + re-open → empty library (in-memory state for this iteration;
  persistence-by-domain-store is filed as T-podcast-gap-004).

Every byte rendered by the Library list comes from the Rust kernel:
  Swift → KernelBridge.podcastSubscribe → nmp_app_podcast_subscribe →
  Mutex<Vec<PodcastRecord>> in podcast-core domain types →
  nmp_app_podcast_snapshot → JSON LibraryView → KernelModel.refresh →
  SwiftUI re-render. No Swift-side state. D0 verified.
