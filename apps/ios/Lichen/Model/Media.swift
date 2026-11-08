import AVFoundation
import Observation
import UIKit

/// Records a voice message: 8 kHz mono 16-bit, what Codec2 takes, 15 seconds at most.
@MainActor @Observable
final class VoiceRecorder {
    static let maxSeconds: TimeInterval = 15

    private(set) var recording = false
    private(set) var elapsed: TimeInterval = 0
    var denied = false
    /// The conversation the recording is for (set from the tap, before the permission prompt):
    /// only that conversation shows it and sends it.
    private(set) var chat: ChatID?

    @ObservationIgnored private var recorder: AVAudioRecorder?
    @ObservationIgnored private var timer: Timer?
    @ObservationIgnored private let url = FileManager.default.temporaryDirectory.appendingPathComponent("voice.caf")

    func start(for chat: ChatID) async {
        if recording { return }
        self.chat = chat
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
        r.record(forDuration: Self.maxSeconds)
        recording = true
        elapsed = 0
        timer = Timer.scheduledTimer(withTimeInterval: 0.1, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated {
                guard let self, let r = self.recorder else { return }
                self.elapsed = r.isRecording ? r.currentTime : Self.maxSeconds
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

/// Plays voice messages: Codec2 frames decoded by the core, wrapped in a WAV.
@MainActor @Observable
final class VoicePlayer: NSObject, AVAudioPlayerDelegate {
    /// The message playing, if any.
    private(set) var playing: Int64?

    @ObservationIgnored private var player: AVAudioPlayer?

    func toggle(_ message: StoredMessage) {
        if playing == message.id {
            stop()
            return
        }
        guard case let .voice(_, mode, frames, _, data) = message.message,
              let pcm = decodeVoice(mode: mode, frames: frames, data: data)
        else { return }
        try? AVAudioSession.sharedInstance().setCategory(.playback, mode: .spokenAudio)
        try? AVAudioSession.sharedInstance().setActive(true)
        player = try? AVAudioPlayer(data: Self.wav(pcm))
        player?.delegate = self
        player?.play()
        playing = message.id
    }

    func stop() {
        player?.stop()
        player = nil
        playing = nil
    }

    nonisolated func audioPlayerDidFinishPlaying(_ player: AVAudioPlayer, successfully flag: Bool) {
        MainActor.assumeIsolated { playing = nil }
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

/// "3 fragments", "1 fragment".
func fragmentsText(_ n: UInt8) -> String {
    String(localized: "\(Int(n)) fragments")
}

