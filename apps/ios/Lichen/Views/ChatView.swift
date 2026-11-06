import PhotosUI
import SwiftUI

/// One conversation (mockup ios-discussion).
struct ChatView: View {
    let chat: ChatID
    @Environment(AppModel.self) private var model
    @State private var draft = ""
    @State private var error: String?

    var body: some View {
        VStack(spacing: 0) {
            ScrollViewReader { proxy in
                ScrollView {
                    LazyVStack(spacing: 10) {
                        if isSecured {
                            Label("Encrypted session", systemImage: "lock.fill")
                                .font(.caption)
                                .foregroundStyle(Theme.secondaryText)
                                .padding(.top, 8)
                        }
                        ForEach(messages, id: \.id) { m in
                            MessageView(message: m, isGroup: isGroup).id(m.id)
                        }
                    }
                    .padding(.horizontal, 14)
                    .padding(.vertical, 12)
                }
                .onChange(of: messages.count) {
                    if let last = messages.last { withAnimation { proxy.scrollTo(last.id, anchor: .bottom) } }
                    model.markRead(chat)
                }
            }
            Composer(chat: chat, draft: $draft, error: $error)
        }
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItem(placement: .principal) {
                VStack(spacing: 2) {
                    Text(model.title(of: chat)).font(.headline)
                    Text(subtitle).font(.caption).foregroundStyle(Theme.secondaryText)
                }
            }
        }
        .onAppear {
            model.load(chat)
            model.markRead(chat)
        }
        .alert("Not sent", isPresented: Binding(get: { error != nil }, set: { if !$0 { error = nil } })) {
            Button("OK", role: .cancel) {}
        } message: {
            Text(error ?? "")
        }
    }

    private var messages: [StoredMessage] { model.chats[chat] ?? [] }

    private var isGroup: Bool {
        if case .group = chat { return true }
        return false
    }

    private var isSecured: Bool {
        if case let .contact(key) = chat { return model.secured.contains(key) }
        return false
    }

    private var subtitle: String {
        switch chat {
        case let .contact(key):
            let hops = hopsBadge(model.contacts.first { $0.key == key }?.hops)
            if let snr = model.lastSnr[key] {
                return String(localized: "\(hops) · SNR \(String(format: "%.0f", snr)) dB")
            }
            return hops
        case let .group(gid):
            guard let g = model.groups.first(where: { $0.gid == gid }) else { return "" }
            let others = max(g.members.count - 1, 0)
            if Int(g.keysKnown) < others {
                return String(localized: "\(String(g.members.count)) members · keys \(String(g.keysKnown))/\(String(others))")
            }
            return String(localized: "\(String(g.members.count)) members")
        }
    }
}

private struct MessageView: View {
    let message: StoredMessage
    let isGroup: Bool
    @Environment(AppModel.self) private var model

    var body: some View {
        VStack(alignment: message.outgoing ? .trailing : .leading, spacing: 3) {
            if isGroup, !message.outgoing, let a = message.author {
                Text(model.name(of: a)).font(.caption.weight(.semibold)).foregroundStyle(Theme.secondaryText)
            }
            switch message.message {
            case .voice:
                VoiceBubble(message: message)
            case .image:
                ImageBubble(message: message)
            default:
                Text(message.message.display)
                    .padding(.horizontal, 14)
                    .padding(.vertical, 9)
                    .background(message.outgoing ? Theme.accent : Theme.incomingBubble, in: RoundedRectangle(cornerRadius: 18))
                    .foregroundStyle(message.outgoing ? .white : .primary)
            }
            if message.outgoing, let caption {
                Text(caption).font(.caption2).foregroundStyle(Theme.secondaryText)
            }
        }
        .frame(maxWidth: .infinity, alignment: message.outgoing ? .trailing : .leading)
        .padding(message.outgoing ? .leading : .trailing, 60)
    }

    private var caption: String? {
        switch message.status {
        case .received: return nil
        case let .sending(sent, total):
            return String(localized: "Sending \(String(min(Int(sent), Int(total)))) / \(String(total)) fragments")
        case .delivered:
            return String(localized: "Delivered · \(Int(message.fragments)) fragments · \(airtime) s of airtime")
        case let .failed(reason):
            return String(localized: "Not delivered: \(reason.display)")
        case .flooded:
            return String(localized: "Sent to the group")
        }
    }

    private var airtime: String { String(format: "%.1f", Double(message.airtimeMs) / 1000) }
}

private struct Composer: View {
    let chat: ChatID
    @Binding var draft: String
    @Binding var error: String?
    @Environment(AppModel.self) private var model
    @State private var picked: PhotosPickerItem?
    @State private var image: PreparedImage?
    @State private var preparing = false

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            if let line = costLine {
                Text(line).font(.caption2).foregroundStyle(Theme.secondaryText).padding(.leading, 6)
            }
            if model.recorder.recording {
                recordingBar
            } else {
                HStack(spacing: 8) {
                    if isContact {
                        PhotosPicker(selection: $picked, matching: .images) {
                            Image(systemName: preparing ? "hourglass" : "photo")
                                .font(.system(size: 20))
                                .frame(width: 32, height: 36)
                        }
                        .disabled(!model.isConnected || preparing)
                        .accessibilityLabel("Send a photo")
                    }
                    TextField("Message", text: $draft, axis: .vertical)
                        .lineLimit(1...5)
                        .padding(.horizontal, 14)
                        .padding(.vertical, 9)
                        .background(Color(.systemBackground), in: RoundedRectangle(cornerRadius: 19))
                        .overlay(RoundedRectangle(cornerRadius: 19).stroke(Color(.separator)))
                    if isContact && draft.isEmpty {
                        Button {
                            Task { await model.recorder.start() }
                        } label: {
                            Image(systemName: "mic.circle.fill").font(.system(size: 32))
                        }
                        .disabled(!model.isConnected)
                        .accessibilityLabel("Record a voice message")
                    } else {
                        Button {
                            let text = draft.trimmingCharacters(in: .whitespacesAndNewlines)
                            if let e = model.send(text, to: chat) { error = e } else { draft = "" }
                        } label: {
                            Image(systemName: "arrow.up.circle.fill").font(.system(size: 32))
                        }
                        .disabled(draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || !model.isConnected)
                        .accessibilityLabel("Send")
                    }
                }
            }
        }
        .padding(.horizontal, 10)
        .padding(.top, 8)
        .padding(.bottom, 10)
        .background(.bar)
        .onChange(of: picked) {
            guard let item = picked else { return }
            picked = nil
            preparing = true
            Task {
                defer { preparing = false }
                guard let data = try? await item.loadTransferable(type: Data.self),
                      let ui = UIImage(data: data), let (rgba, w, h) = ui.rgba()
                else {
                    error = LichenError.BadImage.display
                    return
                }
                do {
                    image = try await Task.detached { try prepareImage(rgba: rgba, width: w, height: h) }.value
                } catch {
                    self.error = LichenError.BadImage.display
                }
            }
        }
        .sheet(item: Binding(get: { image.map(ImageDraft.init) }, set: { image = $0?.image })) { draft in
            SendImageSheet(chat: chat, image: draft.image) { e in
                image = nil
                error = e
            }
            .presentationDetents([.medium])
        }
        .alert("Microphone access is off", isPresented: Binding(get: { model.recorder.denied }, set: { model.recorder.denied = $0 })) {
            Button("OK", role: .cancel) {}
        } message: {
            Text("Allow it in Settings to record voice messages.")
        }
    }

    private var isContact: Bool {
        if case .contact = chat { return true }
        return false
    }

    /// While recording: time, cost so far, cancel, send.
    private var recordingBar: some View {
        let seconds = model.recorder.elapsed
        // 1200 bit/s: 150 bytes a second, plus the 8-byte header.
        let estimate = model.estimate(chat, datagramLen: 8 + UInt32(seconds * 150))
        return HStack(spacing: 12) {
            Button {
                _ = model.recorder.stop(keep: false)
            } label: {
                Image(systemName: "xmark.circle.fill").font(.system(size: 28)).foregroundStyle(Theme.secondaryText)
            }
            .accessibilityLabel("Cancel")
            Circle().fill(Theme.danger).frame(width: 10, height: 10)
            VStack(alignment: .leading, spacing: 1) {
                Text("\(clock(seconds)) / \(clock(VoiceRecorder.maxSeconds))")
                    .font(.subheadline.monospacedDigit().weight(.semibold))
                if let e = estimate {
                    Text("\(fragmentsText(e.fragments)) · ≈ \(String(format: "%.0f", Double(e.airtimeMs) / 1000)) s of airtime")
                        .font(.caption2)
                        .foregroundStyle(Theme.secondaryText)
                }
            }
            Spacer()
            Button {
                guard let pcm = model.recorder.stop(keep: true) else { return }
                let voice = prepareVoice(pcm: pcm)
                if let e = model.sendVoice(voice, to: chat) { error = e }
            } label: {
                Image(systemName: "arrow.up.circle.fill").font(.system(size: 32))
            }
            .accessibilityLabel("Send")
        }
        .padding(.horizontal, 6)
        .frame(minHeight: 40)
    }

    private var costLine: String? {
        if case .group = chat { return String(localized: "Flooded to every member, without acknowledgement") }
        guard let e = model.estimate(chat, text: draft) else { return nil }
        let s = String(format: "%.1f", Double(e.airtimeMs) / 1000)
        return String(localized: "Estimated cost: \(Int(e.fragments)) fragments · ≈ \(s) s of airtime")
    }
}

private struct ImageDraft: Identifiable {
    let image: PreparedImage
    var id: Int { image.full.hashValue }
}

/// The thumbnail and what it costs, before sending.
private struct SendImageSheet: View {
    let chat: ChatID
    let image: PreparedImage
    let done: (String?) -> Void
    @Environment(AppModel.self) private var model

    var body: some View {
        VStack(spacing: 14) {
            if let ui = UIImage(data: image.full) {
                Image(uiImage: ui)
                    .resizable()
                    .interpolation(.medium)
                    .scaledToFit()
                    .frame(maxHeight: 180)
                    .clipShape(RoundedRectangle(cornerRadius: 12))
            }
            if let e = model.estimate(chat, datagramLen: imageDatagramLen(image: image)) {
                Text("Preview: \(image.preview.count) bytes · \(fragmentsText(e.fragments)) · ≈ \(String(format: "%.1f", Double(e.airtimeMs) / 1000)) s of airtime")
                    .font(.subheadline)
            }
            Text("The full thumbnail (\(image.full.count) bytes) goes out only if they ask for it.")
                .font(.caption)
                .foregroundStyle(Theme.secondaryText)
                .multilineTextAlignment(.center)
            HStack {
                Button("Cancel", role: .cancel) { done(nil) }
                    .buttonStyle(.bordered)
                Button("Send preview") { done(model.sendImage(image, to: chat)) }
                    .buttonStyle(.borderedProminent)
            }
        }
        .padding(20)
    }
}

/// Play button, length, codec.
private struct VoiceBubble: View {
    let message: StoredMessage
    @Environment(AppModel.self) private var model

    var body: some View {
        if case let .voice(_, mode, frames, durationMs, data) = message.message {
            // The core says whether this build plays the mode (700C waits for ADR-0012).
            let playable = decodeVoice(mode: mode, frames: 0, data: Data()) != nil
            HStack(spacing: 10) {
                Button {
                    model.player.toggle(message)
                } label: {
                    Image(systemName: model.player.playing == message.id ? "stop.fill" : "play.fill")
                        .font(.system(size: 16, weight: .bold))
                        .frame(width: 34, height: 34)
                        .background(message.outgoing ? .white.opacity(0.25) : Theme.accent.opacity(0.15), in: Circle())
                }
                .disabled(!playable)
                .accessibilityLabel(model.player.playing == message.id ? "Stop" : "Play")
                Waveform(seed: data.prefix(32), bars: 22)
                    .frame(width: 110, height: 22)
                VStack(alignment: .leading, spacing: 0) {
                    Text(clock(TimeInterval(durationMs) / 1000)).font(.subheadline.monospacedDigit().weight(.semibold))
                    Text(playable ? "Codec2" : String(localized: "Codec2 700C: not playable yet"))
                        .font(.caption2)
                        .opacity(0.8)
                }
            }
            .padding(.horizontal, 10)
            .padding(.vertical, 8)
            .background(message.outgoing ? Theme.accent : Theme.incomingBubble, in: RoundedRectangle(cornerRadius: 18))
            .foregroundStyle(message.outgoing ? .white : .primary)
        }
    }
}

/// Bars drawn from the Codec2 bits: a stable, decorative waveform.
private struct Waveform: View {
    let seed: Data
    let bars: Int

    var body: some View {
        GeometryReader { geo in
            HStack(alignment: .center, spacing: 2) {
                ForEach(0..<bars, id: \.self) { i in
                    let b = seed.isEmpty ? 128 : Int(seed[seed.startIndex + i % seed.count])
                    Capsule()
                        .frame(width: max(1, (geo.size.width - CGFloat(bars - 1) * 2) / CGFloat(bars)),
                               height: max(3, geo.size.height * CGFloat(b % 100 + 20) / 120))
                }
            }
            .frame(maxHeight: .infinity)
        }
        .opacity(0.7)
    }
}

/// The thumbnail; a received preview offers its full variant.
private struct ImageBubble: View {
    let message: StoredMessage
    @Environment(AppModel.self) private var model
    @State private var error: String?

    var body: some View {
        if case let .image(_, _, full, width, height, fullLen, data) = message.message {
            VStack(alignment: message.outgoing ? .trailing : .leading, spacing: 6) {
                Group {
                    if let ui = UIImage(data: data) {
                        Image(uiImage: ui).resizable().interpolation(full ? .medium : .high)
                    } else {
                        Color(.secondarySystemBackground)
                    }
                }
                .aspectRatio(CGFloat(max(width, 1)) / CGFloat(max(height, 1)), contentMode: .fit)
                .frame(width: 220)
                .blur(radius: full ? 0 : 1.5)
                .clipShape(RoundedRectangle(cornerRadius: 14))
                if !message.outgoing && !full && fullLen > 0 {
                    if model.requestedFull.contains(message.id) {
                        Label("Full image requested…", systemImage: "hourglass")
                            .font(.caption)
                            .foregroundStyle(Theme.secondaryText)
                    } else {
                        Button {
                            error = model.requestFull(message)
                        } label: {
                            Label(fullLabel(Int(fullLen)), systemImage: "arrow.down.circle")
                                .font(.caption.weight(.semibold))
                        }
                        .buttonStyle(.bordered)
                        .disabled(!model.isConnected)
                    }
                } else if !message.outgoing && !full {
                    Text("Preview").font(.caption2).foregroundStyle(Theme.secondaryText)
                }
            }
            .alert("Not sent", isPresented: Binding(get: { error != nil }, set: { if !$0 { error = nil } })) {
                Button("OK", role: .cancel) {}
            } message: {
                Text(error ?? "")
            }
        }
    }

    private func fullLabel(_ bytes: Int) -> String {
        guard case let .contact(key) = message.chat,
              let e = try? model.core.estimateDatagram(to: key, len: UInt32(21 + bytes))
        else { return String(localized: "Load the full image") }
        return String(localized: "Full image: \(fragmentsText(e.fragments)), ≈ \(String(format: "%.0f", Double(e.airtimeMs) / 1000)) s from their node")
    }
}
