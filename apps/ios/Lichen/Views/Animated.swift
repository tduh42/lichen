import ImageIO
import Lottie
import SwiftUI
import UIKit

/// The animated stickers the app ships (protocol.md §6.3, ADR-0025): Noto Animated Emoji as
/// Lottie animations, stored in raw DEFLATE, found by their emoji.
@MainActor
final class Stickers {
    static let shared = Stickers()

    struct Entry: Decodable, Hashable {
        let emoji: String
        let file: String
        let group: String
    }

    let all: [Entry]
    private let dir: URL?
    private var cache: [String: LottieAnimation] = [:]

    private init() {
        dir = Bundle.main.url(forResource: "noto", withExtension: nil)
        if let dir, let data = try? Data(contentsOf: dir.appendingPathComponent("manifest.json")),
           let list = try? JSONDecoder().decode([Entry].self, from: data) {
            all = list
        } else {
            all = []
        }
    }

    /// The groups of the picker, in order.
    var groups: [(name: String, entries: [Entry])] {
        var out: [(name: String, entries: [Entry])] = []
        for e in all {
            if let i = out.firstIndex(where: { $0.name == e.group }) {
                out[i].entries.append(e)
            } else {
                out.append((e.group, [e]))
            }
        }
        return out
    }

    /// Without the variation selector: "❤" and "❤️" are the same sticker.
    private static func key(_ emoji: String) -> String {
        emoji.replacingOccurrences(of: "\u{FE0F}", with: "")
    }

    /// Already parsed: shown at once.
    func cached(_ emoji: String) -> LottieAnimation? { cache[Self.key(emoji)] }

    /// The animation of `emoji`, read and parsed off the main thread (tens of milliseconds
    /// each), then kept.
    func animation(_ emoji: String) async -> LottieAnimation? {
        let k = Self.key(emoji)
        if let a = cache[k] { return a }
        guard let dir, let entry = all.first(where: { Self.key($0.emoji) == k }) else { return nil }
        let url = dir.appendingPathComponent(entry.file)
        let a = await Task.detached(priority: .userInitiated) { () -> LottieAnimation? in
            guard let packed = try? Data(contentsOf: url),
                  // Apple's "zlib" is raw DEFLATE (RFC 1951), as the files are stored.
                  let json = try? (packed as NSData).decompressed(using: .zlib) as Data
            else { return nil }
            return try? LottieAnimation.from(data: json)
        }.value
        if let a { cache[k] = a }
        return a
    }
}

/// A sticker: its animation played three times, again on a tap; the plain emoji when this
/// phone does not have it (a newer set, §6.3).
struct StickerView: View {
    let emoji: String
    var size: CGFloat = 132
    @State private var replay = 0
    @State private var animation: LottieAnimation?
    @State private var looked = false

    var body: some View {
        Group {
            if let a = animation ?? Stickers.shared.cached(emoji) {
                LottieView(animation: a)
                    .playing(loopMode: .repeat(3))
                    .resizable()
                    .id(replay)
            } else if looked {
                Text(verbatim: emoji).font(.system(size: size * 0.7))
            } else {
                Color.clear
            }
        }
        .frame(width: size, height: size)
        .task(id: emoji) {
            animation = await Stickers.shared.animation(emoji)
            looked = true
        }
        .contentShape(Rectangle())
        .onTapGesture { replay += 1 }
        .accessibilityLabel(Text("Sticker \(emoji)"))
    }
}

/// A still sticker for the picker (dozens at once would cost too much animating).
struct StickerThumb: View {
    let emoji: String
    var size: CGFloat = 56
    @State private var animation: LottieAnimation?

    var body: some View {
        Group {
            if let a = animation ?? Stickers.shared.cached(emoji) {
                LottieView(animation: a)
                    .currentProgress(0.4)
                    .resizable()
            } else {
                // The emoji itself while its animation is read.
                Text(verbatim: emoji).font(.system(size: size * 0.6))
            }
        }
        .frame(width: size, height: size)
        .task(id: emoji) { animation = await Stickers.shared.animation(emoji) }
    }
}

/// The sticker picker: tap one to send it.
struct StickerSheet: View {
    let send: (String) -> Void
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 14) {
                    ForEach(Stickers.shared.groups, id: \.name) { group in
                        SectionLabel("\(Self.title(group.name))")
                        LazyVGrid(columns: Array(repeating: GridItem(.flexible(), spacing: 6), count: 5), spacing: 6) {
                            ForEach(group.entries, id: \.self) { e in
                                Button {
                                    send(e.emoji)
                                    dismiss()
                                } label: {
                                    StickerThumb(emoji: e.emoji)
                                        .frame(maxWidth: .infinity, minHeight: 64)
                                        .contentShape(Rectangle())
                                }
                                .buttonStyle(.plain)
                                .accessibilityLabel(Text("Sticker \(e.emoji)"))
                            }
                        }
                    }
                    Text("A sticker is one radio packet: both phones already have its animation, so it plays even off-grid.")
                        .font(.system(.caption))
                        .foregroundStyle(Theme.tx2)
                        .fixedSize(horizontal: false, vertical: true)
                        .padding(.top, 6)
                }
                .padding(16)
            }
            .screen()
            .navigationTitle("Stickers")
            .navigationBarTitleDisplayMode(.inline)
            .toolbarBackground(Theme.s1, for: .navigationBar)
            .toolbarBackground(.visible, for: .navigationBar)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } }
            }
        }
    }

    private static func title(_ group: String) -> String {
        switch group {
        case "faces": String(localized: "Faces")
        case "hands": String(localized: "Hands")
        case "hearts": String(localized: "Hearts")
        case "celebrations": String(localized: "Celebrations")
        case "outdoors": String(localized: "Outdoors")
        case "animals": String(localized: "Animals")
        default: String(localized: "Signs")
        }
    }
}

/// An animated GIF or WebP, looping (the frames decoded by ImageIO).
struct AnimatedImage: UIViewRepresentable {
    let data: Data

    func makeUIView(context: Context) -> UIImageView {
        let v = UIImageView()
        v.contentMode = .scaleAspectFill
        v.clipsToBounds = true
        v.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        v.setContentCompressionResistancePriority(.defaultLow, for: .vertical)
        v.image = Self.image(data)
        return v
    }

    func updateUIView(_ v: UIImageView, context: Context) {}

    static func image(_ data: Data) -> UIImage? {
        guard let src = CGImageSourceCreateWithData(data as CFData, nil) else { return nil }
        let n = CGImageSourceGetCount(src)
        guard n > 1 else { return UIImage(data: data) }
        var frames: [UIImage] = []
        var total = 0.0
        for i in 0..<n {
            guard let cg = CGImageSourceCreateImageAtIndex(src, i, nil) else { continue }
            frames.append(UIImage(cgImage: cg))
            total += delay(src, i)
        }
        return UIImage.animatedImage(with: frames, duration: total)
    }

    /// The pixel size of the first frame: the shape a GIF message carries (§6.3).
    static func size(_ data: Data) -> (UInt16, UInt16)? {
        guard let src = CGImageSourceCreateWithData(data as CFData, nil),
              let props = CGImageSourceCopyPropertiesAtIndex(src, 0, nil) as? [CFString: Any],
              let w = props[kCGImagePropertyPixelWidth] as? Int, let h = props[kCGImagePropertyPixelHeight] as? Int
        else { return nil }
        return (UInt16(clamping: w), UInt16(clamping: h))
    }

    private static func delay(_ src: CGImageSource, _ i: Int) -> Double {
        let props = CGImageSourceCopyPropertiesAtIndex(src, i, nil) as? [CFString: Any] ?? [:]
        for key in [kCGImagePropertyWebPDictionary, kCGImagePropertyGIFDictionary] {
            guard let d = props[key] as? [CFString: Any] else { continue }
            for k in [kCGImagePropertyWebPUnclampedDelayTime, kCGImagePropertyGIFUnclampedDelayTime,
                      kCGImagePropertyWebPDelayTime, kCGImagePropertyGIFDelayTime] {
                if let v = d[k] as? Double, v > 0.01 { return v }
            }
        }
        return 0.1
    }
}

/// A GIF in the conversation (§6.3): loaded from GIPHY by its id, on a tap unless GIFs load
/// automatically (or through Tor); its title until then, and its source.
struct GifCard: View {
    let message: StoredMessage
    @Environment(AppModel.self) private var model
    @State private var data: Data?
    @State private var loading = false
    @State private var failed = false

    var body: some View {
        if case let .gif(_, provider, width, height, id, title) = message.message {
            VStack(alignment: message.outgoing ? .trailing : .leading, spacing: 3) {
                ZStack {
                    Theme.s2
                    if let data {
                        AnimatedImage(data: data)
                    } else if provider != 1 {
                        placeholder(title, line: String(localized: "A GIF from a source this app does not know"))
                    } else if loading {
                        ProgressView()
                    } else {
                        Button { Task { await load(id) } } label: {
                            placeholder(title, line: failed ? String(localized: "Could not load it · Try again")
                                        : String(localized: "Tap to load from GIPHY"))
                        }
                        .buttonStyle(.plain)
                    }
                }
                .frame(width: 220, height: Self.height(width, height))
                .clipShape(RoundedRectangle(cornerRadius: 16))
                .accessibilityElement(children: .combine)
                .accessibilityLabel(Text(title.isEmpty ? String(localized: "GIF") : String(localized: "GIF: \(title)")))
                Text(verbatim: "GIPHY").font(.system(.caption2, weight: .semibold)).foregroundStyle(Theme.tx2)
            }
            .task(id: id) {
                if provider == 1, data == nil, let cached = model.cachedGif(id) {
                    data = cached
                } else if provider == 1, data == nil, model.gifsLoadThemselves {
                    await load(id)
                }
            }
        }
    }

    private func placeholder(_ title: String, line: String) -> some View {
        VStack(spacing: 4) {
            Text(title.isEmpty ? "GIF" : title).font(.system(.subheadline, weight: .semibold)).foregroundStyle(Theme.tx)
                .lineLimit(2).multilineTextAlignment(.center)
            Text(line).font(.system(.caption)).foregroundStyle(Theme.tx2).multilineTextAlignment(.center)
        }
        .padding(12)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .contentShape(Rectangle())
    }

    private func load(_ id: String) async {
        loading = true
        failed = false
        data = await model.loadGif(id)
        failed = data == nil
        loading = false
    }

    /// The card's height for a GIF of `w` × `h`, 220 points wide; square when unknown.
    static func height(_ w: UInt16, _ h: UInt16) -> CGFloat {
        guard w > 0, h > 0 else { return 220 }
        return min(300, max(90, 220 * CGFloat(h) / CGFloat(w)))
    }
}
