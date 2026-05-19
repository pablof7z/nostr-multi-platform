import AVFoundation
import CryptoKit
import Foundation
import Network
import os

/// Extracts a one-peak-per-second amplitude envelope from a podcast audio
/// stream using AVAssetReader. The output is a `[Float]` of length
/// ~`durationSeconds`, normalized to 0...1.
///
/// Tradeoffs:
/// - AVAssetReader needs the full asset bytes; for a 1h podcast that's
///   several tens of MB. We gate first-time extraction on Wi-Fi to avoid
///   surprising cellular users, and cache the result so repeat plays of
///   the same episode are free.
/// - The reader runs at background priority and is cancellable, so
///   playback is never blocked.
/// - Failure is silent: when extraction can't complete (cellular, asset
///   refuses to seek, format unsupported, etc.) the listening view falls
///   back to plain minute-peg markers.
enum WaveformExtractor {
    private static let logger = Logger(subsystem: "com.highlighter.app", category: "Waveform")

    /// Best-effort fetch with cache. Returns nil if extraction was skipped or
    /// failed for any reason — callers must tolerate the absence of peaks.
    static func peaks(forAudioURL url: URL, durationSeconds: TimeInterval) async -> [Float]? {
        if let cached = WaveformCache.read(for: url) {
            return cached
        }

        guard isWiFiAvailable() else {
            logger.info("waveform extraction skipped — not on Wi-Fi")
            return nil
        }

        let durationGuard = durationSeconds.isFinite && durationSeconds > 0 ? durationSeconds : 0
        let buckets = max(60, Int(durationGuard.rounded()))

        do {
            let peaks = try await extractPeaks(from: url, bucketCount: buckets)
            WaveformCache.write(peaks, for: url)
            return peaks
        } catch {
            logger.error("waveform extraction failed: \(error.localizedDescription, privacy: .public)")
            return nil
        }
    }

    private static func extractPeaks(from url: URL, bucketCount: Int) async throws -> [Float] {
        let asset = AVURLAsset(url: url)

        guard let track = try await asset.loadTracks(withMediaType: .audio).first else {
            throw ExtractError.noAudioTrack
        }

        let reader = try AVAssetReader(asset: asset)
        let outputSettings: [String: Any] = [
            AVFormatIDKey: kAudioFormatLinearPCM,
            AVLinearPCMBitDepthKey: 16,
            AVLinearPCMIsFloatKey: false,
            AVLinearPCMIsBigEndianKey: false,
            AVLinearPCMIsNonInterleaved: false
        ]
        let output = AVAssetReaderTrackOutput(track: track, outputSettings: outputSettings)
        output.alwaysCopiesSampleData = false
        guard reader.canAdd(output) else {
            throw ExtractError.cantAddOutput
        }
        reader.add(output)

        let duration = try await asset.load(.duration).seconds
        guard duration.isFinite, duration > 0 else {
            throw ExtractError.invalidDuration
        }

        let formatDescriptions = try await track.load(.formatDescriptions)
        guard let cmFormat = formatDescriptions.first,
              let asbdPtr = CMAudioFormatDescriptionGetStreamBasicDescription(cmFormat) else {
            throw ExtractError.noFormatDescription
        }
        let sampleRate = asbdPtr.pointee.mSampleRate
        let channelCount = max(Int(asbdPtr.pointee.mChannelsPerFrame), 1)
        guard sampleRate > 0 else {
            throw ExtractError.invalidSampleRate
        }
        let totalSamples = Int(duration * sampleRate)
        let samplesPerBucket = max(totalSamples / bucketCount, 1)

        guard reader.startReading() else {
            throw ExtractError.readerStartFailed(reader.error)
        }

        var peaks: [Float] = []
        peaks.reserveCapacity(bucketCount)
        var bucketPeak: Int16 = 0
        var bucketSampleCount = 0
        var maxObserved: Int16 = 1

        while reader.status == .reading {
            try Task.checkCancellation()
            guard let sampleBuffer = output.copyNextSampleBuffer(),
                  let blockBuffer = CMSampleBufferGetDataBuffer(sampleBuffer) else {
                break
            }

            let length = CMBlockBufferGetDataLength(blockBuffer)
            var data = Data(count: length)
            data.withUnsafeMutableBytes { (ptr: UnsafeMutableRawBufferPointer) -> Void in
                guard let base = ptr.baseAddress else { return }
                CMBlockBufferCopyDataBytes(blockBuffer, atOffset: 0, dataLength: length, destination: base)
            }
            CMSampleBufferInvalidate(sampleBuffer)

            data.withUnsafeBytes { (raw: UnsafeRawBufferPointer) in
                let int16Buffer = raw.bindMemory(to: Int16.self)
                var idx = 0
                while idx < int16Buffer.count {
                    var frameMax: Int16 = 0
                    for ch in 0..<channelCount where idx + ch < int16Buffer.count {
                        let sample = abs(int16Buffer[idx + ch])
                        if sample > frameMax { frameMax = sample }
                    }
                    if frameMax > bucketPeak { bucketPeak = frameMax }
                    if frameMax > maxObserved { maxObserved = frameMax }
                    bucketSampleCount += 1
                    idx += channelCount

                    if bucketSampleCount >= samplesPerBucket {
                        peaks.append(Float(bucketPeak))
                        bucketPeak = 0
                        bucketSampleCount = 0
                    }
                }
            }
        }

        if bucketSampleCount > 0 {
            peaks.append(Float(bucketPeak))
        }

        if reader.status == .failed {
            throw ExtractError.readerFailed(reader.error)
        }

        let denominator = Float(maxObserved)
        return peaks.map { min(1, $0 / denominator) }
    }

    private static func isWiFiAvailable() -> Bool {
        let monitor = NWPathMonitor()
        let semaphore = DispatchSemaphore(value: 0)
        let result = OSAllocatedUnfairLock(initialState: false)
        let queue = DispatchQueue(label: "com.highlighter.waveform.path")
        monitor.pathUpdateHandler = { path in
            let onWifi = path.status == .satisfied && path.usesInterfaceType(.wifi)
            result.withLock { $0 = onWifi }
            semaphore.signal()
        }
        monitor.start(queue: queue)
        _ = semaphore.wait(timeout: .now() + .milliseconds(250))
        monitor.cancel()
        return result.withLock { $0 }
    }

    enum ExtractError: Error {
        case noAudioTrack
        case cantAddOutput
        case invalidDuration
        case noFormatDescription
        case invalidSampleRate
        case readerStartFailed(Error?)
        case readerFailed(Error?)
    }
}

/// Cache layer for extracted waveforms. Stored as raw `Float` little-endian
/// bytes (4 bytes per peak) under Library/Caches/highlighter/waveforms,
/// keyed by SHA-256 of the audio URL string. A 1-hour podcast at one peak
/// per second is 14 KB — cheap to keep around indefinitely.
enum WaveformCache {
    private static let logger = Logger(subsystem: "com.highlighter.app", category: "WaveformCache")

    static func read(for url: URL) -> [Float]? {
        guard let path = filePath(for: url), FileManager.default.fileExists(atPath: path.path) else {
            return nil
        }
        guard let data = try? Data(contentsOf: path) else { return nil }
        let count = data.count / MemoryLayout<Float>.size
        var peaks = [Float](repeating: 0, count: count)
        _ = peaks.withUnsafeMutableBytes { dst in
            data.copyBytes(to: dst, count: data.count)
        }
        return peaks
    }

    static func write(_ peaks: [Float], for url: URL) {
        guard let path = filePath(for: url) else { return }
        do {
            try FileManager.default.createDirectory(
                at: path.deletingLastPathComponent(),
                withIntermediateDirectories: true
            )
            let data = peaks.withUnsafeBufferPointer { buf in
                Data(buffer: buf)
            }
            try data.write(to: path, options: .atomic)
        } catch {
            logger.error("waveform cache write failed: \(error.localizedDescription, privacy: .public)")
        }
    }

    private static func filePath(for url: URL) -> URL? {
        guard let dir = FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask).first else {
            return nil
        }
        let hash = SHA256.hash(data: Data(url.absoluteString.utf8))
        let name = hash.compactMap { String(format: "%02x", $0) }.joined()
        return dir
            .appendingPathComponent("highlighter", isDirectory: true)
            .appendingPathComponent("waveforms", isDirectory: true)
            .appendingPathComponent(name + ".bin")
    }
}
