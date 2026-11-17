import AVFoundation
import Observation
import ImageIO
import UIKit

/// Records a voice message: 8 kHz mono 16-bit, what Codec2 takes, 15 seconds at most.
@MainActor @Observable
final class VoiceRecorder {
    static let maxSeconds: TimeInterval = 15

    private(set) var recording = false
    private(set) var elapsed: TimeInterval = 0
    /// The longest this recording may be: 15 s, 6 s for a group (ADR-0035).
    private(set) var limit: TimeInterval = VoiceRecorder.maxSeconds
    var denied = false
    /// The conversation the recording is for (set from the tap, before the permission prompt):
    /// only that conversation shows it and sends it.
    private(set) var chat: ChatID?

    @ObservationIgnored private var recorder: AVAudioRecorder?
    @ObservationIgnored private var timer: Timer?
    @ObservationIgnored private let url = FileManager.default.temporaryDirectory.appendingPathComponent("voice.caf")

    func start(for chat: ChatID, maxSeconds: TimeInterval = VoiceRecorder.maxSeconds) async {
        if recording { return }
        self.chat = chat
        limit = min(maxSeconds, Self.maxSeconds)
        guard await AVAudioApplication.requestRecordPermission() else {
            denied = true
            self.chat = nil
            return
        }
        // The conversation was left while the permission prompt was up.
        guard self.chat == chat else { return }
        let session = AVAudioSession.sharedInstance()
        try? session.setCategory(.playAndRecord, mode: .spokenAudio, options: [.defaultToSpeaker])
        try? session.setActive(true)
        let settings: [String: Any] = [
            AVFormatIDKey: kAudioFormatLinearPCM,
            AVSampleRateKey: 8_000,
            AVNumberOfChannelsKey: 1,
            AVLinearPCMBitDepthKey: 16,
            AVLinearPCMIsFloatKey: false,
            AVLinearPCMIsBigEndianKey: false,
        ]
        guard let r = try? AVAudioRecorder(url: url, settings: settings) else {
            self.chat = nil
            return
        }
        recorder = r
        r.record(forDuration: limit)
        recording = true
        elapsed = 0
        timer = Timer.scheduledTimer(withTimeInterval: 0.1, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated {
                guard let self, let r = self.recorder else { return }
                self.elapsed = r.isRecording ? r.currentTime : self.limit
            }
        }
    }

    /// Conversation `chat` was left: its recording is dropped, never sent elsewhere.
    func cancel(for chat: ChatID) {
        guard self.chat == chat else { return }
        _ = stop(keep: false)
    }

    /// Stops and returns the samples (nil when cancelled or empty).
    func stop(keep: Bool) -> [Int16]? {
        chat = nil
        timer?.invalidate()
        recorder?.stop()
        recorder = nil
        recording = false
        try? AVAudioSession.sharedInstance().setActive(false, options: .notifyOthersOnDeactivation)
        guard keep, let file = try? AVAudioFile(forReading: url, commonFormat: .pcmFormatInt16, interleaved: false),
              let buffer = AVAudioPCMBuffer(pcmFormat: file.processingFormat, frameCapacity: AVAudioFrameCount(file.length)),
              (try? file.read(into: buffer)) != nil,
              let samples = buffer.int16ChannelData?[0], buffer.frameLength > 0
        else { return nil }
        return Array(UnsafeBufferPointer(start: samples, count: Int(buffer.frameLength)))
    }
}

/// Plays voice messages: Codec2 frames decoded by the core, wrapped in a WAV. Follows the
/// position (for the progress bar) and seeks.
@MainActor @Observable
final class VoicePlayer: NSObject, AVAudioPlayerDelegate {
    /// What is playing: a message id, or `review` (a recording not sent yet).
    private(set) var playing: Int64?
    /// Where it is, 0...1.
    private(set) var progress: Double = 0
    /// And in seconds.
    private(set) var position: TimeInterval = 0

    /// The id of a recording being reviewed before it is sent.
    static let review: Int64 = -1

    @ObservationIgnored private var player: AVAudioPlayer?
    @ObservationIgnored private var ticker: Timer?

    /// Plays message `message` from the start, or stops it when it is playing.
    func toggle(_ message: StoredMessage) {
        if playing == message.id {
            stop()
            return
        }
        play(message, from: 0)
    }

    /// Plays message `message` from `fraction` of it.
    func play(_ message: StoredMessage, from fraction: Double) {
        guard case let .voice(_, mode, frames, _, data) = message.message,
              let pcm = decodeVoice(mode: mode, frames: frames, data: data)
        else { return }
        play(pcm: pcm, id: message.id, from: fraction)
    }

    /// Plays 8 kHz samples under `id`.
    func play(pcm: [Int16], id: Int64, from fraction: Double = 0) {
        stop()
        try? AVAudioSession.sharedInstance().setCategory(.playback, mode: .spokenAudio)
        try? AVAudioSession.sharedInstance().setActive(true)
        guard let p = try? AVAudioPlayer(data: Self.wav(pcm)) else { return }
        p.delegate = self
        p.currentTime = max(0, min(1, fraction)) * p.duration
        p.play()
        player = p
        playing = id
        ticker = Timer.scheduledTimer(withTimeInterval: 0.05, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated { self?.tick() }
        }
        tick()
    }

    /// Moves to `fraction` of what is playing.
    func seek(_ fraction: Double) {
        guard let p = player else { return }
        p.currentTime = max(0, min(1, fraction)) * p.duration
        tick()
    }

    func stop() {
        ticker?.invalidate()
        ticker = nil
        player?.stop()
        player = nil
        playing = nil
        progress = 0
        position = 0
    }

    private func tick() {
        guard let p = player, p.duration > 0 else { return }
        position = p.currentTime
        progress = p.currentTime / p.duration
    }

    nonisolated func audioPlayerDidFinishPlaying(_ player: AVAudioPlayer, successfully flag: Bool) {
        MainActor.assumeIsolated { stop() }
    }

    /// 8 kHz mono 16-bit little-endian WAV.
    static func wav(_ pcm: [Int16]) -> Data {
        var d = Data()
        func u32(_ v: UInt32) { withUnsafeBytes(of: v.littleEndian) { d.append(contentsOf: $0) } }
        func u16(_ v: UInt16) { withUnsafeBytes(of: v.littleEndian) { d.append(contentsOf: $0) } }
        let bytes = UInt32(pcm.count * 2)
        d.append(contentsOf: Array("RIFF".utf8)); u32(36 + bytes)
        d.append(contentsOf: Array("WAVEfmt ".utf8)); u32(16); u16(1); u16(1)
        u32(8_000); u32(16_000); u16(2); u16(16)
        d.append(contentsOf: Array("data".utf8)); u32(bytes)
        pcm.withUnsafeBytes { d.append(contentsOf: $0) }
        return d
    }
}


/// An image decoded no larger than `maxPixel` on its longer side, its header read first: one
/// that declares more than 16,384 pixels on a side, or 64 million in all, is refused instead
/// of decoded whole. A received image is a few hundred bytes that can declare any size
/// (review of 2026-11-15, SR-27).
enum BoundedImage {
    static func from(data: Data, maxPixel: Int) -> UIImage? {
        guard let src = CGImageSourceCreateWithData(data as CFData, nil) else { return nil }
        return make(src, maxPixel)
    }

    static func from(url: URL, maxPixel: Int) -> UIImage? {
        guard let src = CGImageSourceCreateWithURL(url as CFURL, nil) else { return nil }
        return make(src, maxPixel)
    }

    private static func make(_ src: CGImageSource, _ maxPixel: Int) -> UIImage? {
        guard let props = CGImageSourceCopyPropertiesAtIndex(src, 0, nil) as? [CFString: Any],
              let w = props[kCGImagePropertyPixelWidth] as? Int,
              let h = props[kCGImagePropertyPixelHeight] as? Int,
              w > 0, h > 0, w <= 16_384, h <= 16_384, w * h <= 64_000_000
        else { return nil }
        let options: [CFString: Any] = [
            kCGImageSourceCreateThumbnailFromImageAlways: true,
            kCGImageSourceCreateThumbnailWithTransform: true,
            kCGImageSourceThumbnailMaxPixelSize: maxPixel,
            kCGImageSourceShouldCacheImmediately: true,
        ]
        guard let cg = CGImageSourceCreateThumbnailAtIndex(src, 0, options as CFDictionary) else { return nil }
        return UIImage(cgImage: cg)
    }
}

/// Images decoded off the main thread, once (review of 2026-11-15, SR-27). A view asks for one
/// in its body: a decoded image comes back at once; otherwise its decoding starts in the
/// background, the view shows its placeholder, and draws again when the image is ready.
@MainActor @Observable
final class DecodedImages {
    static let shared = DecodedImages()

    private var images: [String: UIImage] = [:]
    @ObservationIgnored private var order: [String] = []
    @ObservationIgnored private var pending: Set<String> = []
    /// Bytes that did not decode: not tried again at each redraw.
    @ObservationIgnored private var failed: Set<String> = []
    /// A screen of photos and the viewer; the oldest go first.
    private let keep = 48
    private let keepAnimated = 6

    func image(data: Data, maxPixel: Int) -> UIImage? {
        var h = Hasher()
        data.withUnsafeBytes { h.combine(bytes: $0) }
        return image(key: "d\(h.finalize())/\(data.count)/\(maxPixel)") {
            BoundedImage.from(data: data, maxPixel: maxPixel)
        }
    }

    /// A downloaded file: its content never changes under its path (named by its hash).
    func image(url: URL, maxPixel: Int) -> UIImage? {
        image(key: "u\(url.path)/\(maxPixel)") { BoundedImage.from(url: url, maxPixel: maxPixel) }
    }

    /// A GIF or WebP's frames (AnimatedFrames). Heavier: only the last few are kept.
    func animated(data: Data) -> UIImage? {
        var h = Hasher()
        data.withUnsafeBytes { h.combine(bytes: $0) }
        return image(key: "a\(h.finalize())/\(data.count)") { AnimatedFrames.image(data) }
    }

    private func image(key: String, decode: @escaping @Sendable () -> UIImage?) -> UIImage? {
        if let ui = images[key] { return ui }
        guard !pending.contains(key), !failed.contains(key) else { return nil }
        pending.insert(key)
        Task.detached(priority: .userInitiated) {
            let ui = decode()
            await self.store(key, ui)
        }
        return nil
    }

    private func store(_ key: String, _ ui: UIImage?) {
        pending.remove(key)
        guard let ui else {
            failed.insert(key)
            return
        }
        images[key] = ui
        order.append(key)
        // Frames weigh tens of megabytes: at most a few animations, and `keep` images in all.
        while order.count > keep || order.filter({ $0.hasPrefix("a") }).count > keepAnimated,
              let oldest = order.first(where: { order.count > keep || $0.hasPrefix("a") }) {
            order.removeAll { $0 == oldest }
            images[oldest] = nil
        }
    }
}

extension UIImage {
    /// RGBA8 pixels at most `maxSide` on the longer side (the core makes the thumbnail).
    func rgba(maxSide: CGFloat = 640) -> (Data, UInt32, UInt32)? {
        let scale = min(1, maxSide / max(size.width, size.height))
        let w = max(1, Int((size.width * scale).rounded()))
        let h = max(1, Int((size.height * scale).rounded()))
        var data = Data(count: w * h * 4)
        let drawn: Bool = data.withUnsafeMutableBytes { buf in
            guard let ctx = CGContext(
                data: buf.baseAddress, width: w, height: h, bitsPerComponent: 8, bytesPerRow: w * 4,
                space: CGColorSpaceCreateDeviceRGB(),
                bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
            ) else { return false }
            UIGraphicsPushContext(ctx)
            ctx.translateBy(x: 0, y: CGFloat(h))
            ctx.scaleBy(x: 1, y: -1)
            draw(in: CGRect(x: 0, y: 0, width: w, height: h))
            UIGraphicsPopContext()
            return true
        }
        return drawn ? (data, UInt32(w), UInt32(h)) : nil
    }
}

/// "0:06".
func clock(_ seconds: TimeInterval) -> String {
    let s = Int(seconds.rounded())
    return String(format: "%d:%02d", s / 60, s % 60)
}

/// A disappearing-message timer in words: "5 minutes", "1 week" (§6.5).
func timerLabel(_ seconds: UInt32) -> String {
    let f = DateComponentsFormatter()
    f.unitsStyle = .full
    f.maximumUnitCount = 1
    f.allowedUnits = [.minute, .hour, .day, .weekOfMonth]
    return f.string(from: TimeInterval(seconds)) ?? "\(seconds) s"
}

/// "3 fragments", "1 fragment".
func fragmentsText(_ n: UInt8) -> String {
    String(localized: "\(Int(n)) fragments")
}

