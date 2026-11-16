import QuickLook
import SwiftUI

/// Chat info › Media, files and voice (ADR-0034): what a conversation holds, the newest first.
/// Photos in a grid (the best version on this phone), files and voice messages in lists.
struct ChatMediaView: View {
    let chat: ChatID
    @Environment(AppModel.self) private var model
    @State private var kind = Kind.photos
    @State private var items: [StoredMessage] = []
    @State private var viewing: StoredMessage?
    @State private var opened: URL?

    enum Kind: Hashable { case photos, files, voice }

    var body: some View {
        VStack(spacing: 0) {
            Picker("Show", selection: $kind) {
                Text("Photos").tag(Kind.photos)
                Text("Files").tag(Kind.files)
                Text("Voice").tag(Kind.voice)
            }
            .pickerStyle(.segmented)
            .padding(.horizontal, 16)
            .padding(.vertical, 10)
            ScrollView {
                let shown = items.filter { Self.kind(of: $0) == kind }
                if shown.isEmpty {
                    Text(empty)
                        .font(.system(.subheadline))
                        .foregroundStyle(Theme.tx2)
                        .padding(.top, 40)
                } else if kind == .photos {
                    LazyVGrid(columns: [GridItem(.adaptive(minimum: 104), spacing: 4)], spacing: 4) {
                        ForEach(shown, id: \.id) { m in
                            Button { viewing = m } label: { thumbnail(m) }
                                .buttonStyle(.plain)
                                .accessibilityLabel(String(localized: "Photo, \(m.date.formatted(date: .abbreviated, time: .shortened))"))
                        }
                    }
                    .padding(.horizontal, 4)
                } else {
                    LazyVStack(alignment: .leading, spacing: 12) {
                        ForEach(shown, id: \.id) { m in row(m) }
                    }
                    .padding(.horizontal, 16)
                }
            }
        }
        .screen()
        .navigationTitle("Media, files and voice")
        .navigationBarTitleDisplayMode(.inline)
        .onAppear { items = model.chatMedia(chat) }
        .fullScreenCover(item: $viewing) { PhotoViewer(message: $0) }
        .quickLookPreview($opened)
    }

    private var empty: String {
        switch kind {
        case .photos: String(localized: "No photos in this conversation.")
        case .files: String(localized: "No files in this conversation.")
        case .voice: String(localized: "No voice messages in this conversation.")
        }
    }

    static func kind(of m: StoredMessage) -> Kind? {
        switch m.message {
        case .image: .photos
        case .file: .files
        case .voice: .voice
        case let .announce(_, _, kind, _, _, _, _): kind == 1 ? .photos : kind == 2 ? .voice : .files
        default: nil
        }
    }

    /// The best picture on this phone: a download over the bridge, else the LoRa image.
    private func thumbnail(_ m: StoredMessage) -> some View {
        let ui: UIImage? = switch m.message {
        case let .image(_, mediaId, _, _, _, _, data):
            downloaded(model.announcement(for: mediaId, in: m.chat)) ?? DecodedImages.shared.image(data: data, maxPixel: 240)
        case .announce: downloaded(m)
        default: nil
        }
        return Color.clear
            .aspectRatio(1, contentMode: .fit)
            .overlay {
                if let ui {
                    Image(uiImage: ui).resizable().scaledToFill()
                } else {
                    Image(systemName: "photo").foregroundStyle(Theme.tx2)
                }
            }
            .background(Theme.s2)
            .clipped()
    }

    private func downloaded(_ announce: StoredMessage?) -> UIImage? {
        guard let a = announce, case let .announce(_, _, _, hash, _, _, _) = a.message,
              let url = model.downloads[hash] else { return nil }
        return DecodedImages.shared.image(url: url, maxPixel: 480)
    }

    @ViewBuilder private func row(_ m: StoredMessage) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(m.date.formatted(date: .abbreviated, time: .shortened))
                .font(.system(.caption))
                .foregroundStyle(Theme.tx2)
            switch m.message {
            case .voice:
                VoiceNote(message: m)
            case let .file(_, name, data):
                Button { opened = model.openable(name: name, data: data) } label: {
                    FileRow(name: name, size: data.count, badge: TransportBadge(kind: .lora, label: "LoRa"))
                }
                .buttonStyle(.plain)
            case let .announce(_, _, _, hash, size, _, name):
                let file = model.downloads[hash]
                Button { if let file { opened = model.openable(name: name, file: file) } } label: {
                    FileRow(name: name, size: Int(size),
                            badge: TransportBadge(kind: .neutral, label: file == nil ? String(localized: "Not downloaded") : String(localized: "Internet")))
                }
                .buttonStyle(.plain)
                .disabled(file == nil)
            default:
                EmptyView()
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(12)
        .background(Theme.s1, in: RoundedRectangle(cornerRadius: 14))
    }
}

extension StoredMessage: Identifiable {}

/// Settings › Storage (ADR-0034): what each conversation takes on this phone, and media older
/// than a month, three months or a year removed as "delete for me" removes them.
struct StorageView: View {
    @Environment(AppModel.self) private var model
    @State private var usage: [(chat: ChatID, title: String, messages: Int, media: Int, bytes: Int64)] = []
    @State private var clearing: Int?
    @State private var cleared: Int?

    var body: some View {
        List {
            Section {
                ForEach(usage, id: \.chat) { u in
                    HStack {
                        VStack(alignment: .leading, spacing: 2) {
                            Text(u.title).font(.system(.body, weight: .semibold)).lineLimit(1)
                            Text(String(localized: "\(u.messages) messages · \(u.media) media"))
                                .font(.system(.footnote))
                                .foregroundStyle(Theme.tx2)
                        }
                        Spacer()
                        Text(ByteCountFormatter.string(fromByteCount: u.bytes, countStyle: .file))
                            .font(.system(.subheadline).monospacedDigit())
                            .foregroundStyle(Theme.tx2)
                    }
                    .listRowBackground(Theme.s1)
                }
                if usage.isEmpty {
                    Text("No conversations yet.").foregroundStyle(Theme.tx2).listRowBackground(Theme.s1)
                }
            } header: {
                Text(String(localized: "In all: \(ByteCountFormatter.string(fromByteCount: usage.reduce(0) { $0 + $1.bytes }, countStyle: .file))"))
            }
            Section {
                ForEach([30, 90, 365], id: \.self) { days in
                    Button(role: .destructive) { clearing = days } label: { Text(Self.olderThan(days)) }
                        .listRowBackground(Theme.s1)
                }
            } header: {
                Text("Clear media")
            } footer: {
                Text("Photos, files and voice messages older than that are removed from this phone, as “Delete for me” does: their place in the conversation goes too. The other phones keep theirs.")
            }
        }
        .scrollContentBackground(.hidden)
        .background(Theme.bg)
        .navigationTitle("Storage")
        .navigationBarTitleDisplayMode(.inline)
        .onAppear(perform: refresh)
        .confirmationDialog(clearing.map { Self.olderThan($0) } ?? "", isPresented: Binding(get: { clearing != nil }, set: { if !$0 { clearing = nil } }),
                            titleVisibility: .visible) {
            Button("Remove", role: .destructive) {
                if let days = clearing { cleared = model.clearMedia(olderThanDays: days) }
                clearing = nil
                refresh()
            }
        } message: {
            Text("This cannot be undone.")
        }
        .alert(String(localized: "\(cleared ?? 0) removed"), isPresented: Binding(get: { cleared != nil }, set: { if !$0 { cleared = nil } })) {
            Button("OK", role: .cancel) {}
        }
    }

    private func refresh() {
        usage = model.storageUsage().map { (chat: $0.usage.chat, title: model.title(of: $0.usage.chat),
                                            messages: Int($0.usage.messages), media: Int($0.usage.media), bytes: $0.bytes) }
            .sorted { $0.bytes > $1.bytes }
    }

    static func olderThan(_ days: Int) -> String {
        switch days {
        case 30: String(localized: "Media older than 30 days")
        case 90: String(localized: "Media older than 90 days")
        default: String(localized: "Media older than 1 year")
        }
    }
}
