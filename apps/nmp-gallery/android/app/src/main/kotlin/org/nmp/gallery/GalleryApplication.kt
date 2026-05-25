package org.nmp.gallery

import android.app.Application

/**
 * Single-process Application — no init beyond what the ViewModel does.
 * The kernel handle is owned by [org.nmp.gallery.bridge.GalleryModel],
 * not by this Application instance, so process recreation cleans up
 * automatically.
 */
class GalleryApplication : Application()
