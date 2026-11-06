import PhotosUI
import SwiftUI

/// One conversation (mockup `chat`).
struct ChatView: View {
    let chat: ChatID
    @Environment(AppModel.self) private var model
    @State private var draft = ""
    @State private var error: String?
    @State private var showingGroup = false
    /// The verification sheet: an empty value for the current identity, else a pending one.
    @State private var verifying: Data?

    var body: some View {
        VStack(spacing: 0) {
            ScrollViewReader { proxy in
                ScrollView {
                    LazyVStack(spacing: 10) {
                        sessionLine
                        ForEach(messages, id: \.id) { m in
                            MessageView(message: m, isGroup: isGroup).id(m.id)
                        }
                        if let failed = lastFailure {
                            FailureBanner(message: failed) { error = model.resend(failed) }
                                .padding(.top, 4)
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
        .screen()
        .navigationBarTitleDisplayMode(.inline)
        .toolbarBackground(Theme.s1, for: .navigationBar)
        .toolbarBackground(.visible, for: .navigationBar)
        .toolbar {
            ToolbarItem(placement: .principal) {
                VStack(spacing: 2) {
                    Text(model.title(of: chat)).font(.system(size: 17, weight: .semibold))
                    Text(subtitle).font(.system(size: 12)).foregroundStyle(Theme.tx2)
                }
            }
            if isGroup {
                ToolbarItem(placement: .topBarTrailing) {
                    Button { showingGroup = true } label: { Image(systemName: "info.circle") }
                        .accessibilityLabel("Group details")
                }
            }
        }
        .navigationDestination(isPresented: $showingGroup) {
            if case let .group(gid) = chat { GroupView(gid: gid) }
        }
        .onAppear {
            model.load(chat)
            model.markRead(chat)
            #if DEBUG
            if model.debugScreen == "group", isGroup { showingGroup = true }
            #endif
        }
        .sheet(isPresented: Binding(get: { verifying != nil }, set: { if !$0 { verifying = nil } })) {
            if case let .contact(key) = chat {
                VerifySheet(node: key, pending: verifying.flatMap { $0.isEmpty ? nil : $0 })
            }
        }
        .alert("Not sent", isPresented: Binding(get: { error != nil }, set: { if !$0 { error = nil } })) {
            Button("OK", role: .cancel) {}
        } message: {
            Text(error ?? "")
        }
    }

    private var messages: [StoredMessage] { model.chats[chat] ?? [] }

    /// The last message we sent, when it failed: a banner offers to send it again.
    private var lastFailure: StoredMessage? {
        guard let last = messages.last(where: { $0.outgoing }), case .failed = last.status else { return nil }
        return last
    }

    private var isGroup: Bool {
        if case .group = chat { return true }
        return false
    }

    @ViewBuilder private var sessionLine: some View {
        if case let .contact(key) = chat {
            if let pending = model.conflicts[key] {
                Banner(kind: .sos, title: String(localized: "\(model.title(of: chat))'s key changed"),
                       message: String(localized: "Their radio now answers with another identity: a reinstalled app, or someone in between. Nothing is sent until you compare safety numbers."),
                       action: (String(localized: "Compare"), { verifying = pending }))
                    .padding(.top, 4)
            } else if let c = model.contact(key), c.identity != nil {
                Button { verifying = Data() } label: {
                    Label(c.verified
                          ? String(localized: "Encrypted session · key verified")
                          : String(localized: "Encrypted session · key to verify"),
                          systemImage: c.verified ? "lock" : "lock.open")
                        .font(.system(size: 12))
                        .foregroundStyle(c.verified ? Theme.tx2 : Theme.warnFg)
                        .labelStyle(TintedIconLabel(tint: c.verified ? Theme.ok : Theme.warn))
                }
                .buttonStyle(.plain)
                .padding(.top, 4)
            } else if model.secured.contains(key) {
                Label("Encrypted session", systemImage: "lock")
                    .font(.system(size: 12))
                    .foregroundStyle(Theme.tx2)
                    .labelStyle(TintedIconLabel(tint: Theme.ok))
                    .padding(.top, 4)
            }
        } else if isGroup {
            Label("Signed group messages, flooded", systemImage: "lock")
                .font(.system(size: 12))
                .foregroundStyle(Theme.tx2)
                .labelStyle(TintedIconLabel(tint: Theme.ok))
                .padding(.top, 4)
        }
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
                return String(localized: "\(g.members.count) members · keys \(Int(g.keysKnown))/\(others)")
            }
            return String(localized: "\(g.members.count) members · flood")
        }
    }
}

/// An icon in its own colour next to the text.
struct TintedIconLabel: LabelStyle {
    let tint: Color

    func makeBody(configuration: Configuration) -> some View {
        HStack(spacing: 6) {
            configuration.icon.foregroundStyle(tint)
            configuration.title
        }
    }
}

/// Why the last message did not go, and a way to send it again (mockup `etats`).
private struct FailureBanner: View {
    let message: StoredMessage
    let resend: () -> Void
    @Environment(AppModel.self) private var model

    var body: some View {
        if case let .failed(reason) = message.status {
            let canResend: Bool = {
                switch message.message {
                case .text, .voice: true
                default: false
                }
            }()
            let action: (label: String, run: () -> Void)? = canResend ? (label: String(localized: "Resend"), run: resend) : nil
            switch reason {
            case .noPath:
                Banner(kind: .warn, title: String(localized: "No path to \(model.title(of: message.chat))"),
                       message: String(localized: "No repeater answered the path discovery. Try again when you are closer or a relay is up."),
                       action: action)
            case .unreachable:
                Banner(kind: .warn, title: String(localized: "\(model.title(of: message.chat)) is unreachable"),
                       message: String(localized: "Their radio did not answer the handshake: off, out of range, or their app is closed."),
                       action: action)
            case .timeout:
                Banner(kind: .sos, title: String(localized: "Send abandoned"),
                       message: String(localized: "\(Int(message.fragments)) fragments, no acknowledgement after several rounds."),
                       action: action)
            case .refused, .tooLarge:
                Banner(kind: .sos, title: String(localized: "Refused"),
                       message: reason.display, action: nil)
            case .interrupted:
                Banner(kind: .warn, title: String(localized: "Not sent"),
                       message: String(localized: "The app stopped before delivery."), action: action)
            case .identityChanged:
                Banner(kind: .sos, title: String(localized: "Not sent: identity changed"),
                       message: String(localized: "Compare safety numbers with \(model.title(of: message.chat)) before accepting their new key."),
                       action: nil)
            }
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
                Text(model.name(of: a)).font(.system(size: 12, weight: .semibold)).foregroundStyle(Theme.tx2)
                    .padding(.horizontal, 6)
            }
            switch message.message {
            case .voice:
                VoiceNote(message: message)
            case .image:
                ImageCard(message: message)
            default:
                Text(message.message.display)
                    .font(.system(size: 16))
                    .padding(.horizontal, 14)
                    .padding(.vertical, 9)
                    .background(message.outgoing ? Theme.actBg : Theme.s2, in: RoundedRectangle(cornerRadius: 18))
                    .foregroundStyle(message.outgoing ? Theme.actFg : Theme.tx)
            }
            if message.outgoing, case let .sending(sent, total) = message.status, total > 1 {
                FragmentProgress(sent: Int(sent), total: Int(total))
                    .frame(width: 220)
                    .padding(.top, 2)
            }
            if message.outgoing, let caption {
                Text(caption).font(.system(size: 11)).foregroundStyle(Theme.tx2)
            }
        }
        .frame(maxWidth: .infinity, alignment: message.outgoing ? .trailing : .leading)
        .padding(message.outgoing ? .leading : .trailing, 64)
    }

    private var caption: String? {
        switch message.status {
        case .received: return nil
        case let .sending(sent, total):
            return total > 1 ? String(localized: "Waiting for the acknowledgement") :
                String(localized: "Sending \(min(Int(sent), Int(total))) / \(Int(total)) fragments")
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

/// Play button (LoRa orange), waveform, length, codec (mockup `chat`).
private struct VoiceNote: View {
    let message: StoredMessage
    @Environment(AppModel.self) private var model

    var body: some View {
        if case let .voice(_, mode, _, durationMs, data) = message.message {
            // The core says whether this build plays the mode.
            let playable = decodeVoice(mode: mode, frames: 0, data: Data()) != nil
            let playing = model.player.playing == message.id
            HStack(spacing: 10) {
                Button { model.player.toggle(message) } label: {
                    Image(systemName: playing ? "stop.fill" : "play.fill")
                        .font(.system(size: 15, weight: .bold))
                        .frame(width: 36, height: 36)
                        .background(Theme.lora, in: Circle())
                        .foregroundStyle(Color(hex: 0x141413))
                }
                .frame(minWidth: 44, minHeight: 44)
                .disabled(!playable)
                .accessibilityLabel(playing ? "Stop" : "Play the voice message")
                Waveform(seed: data.prefix(32), color: message.outgoing ? Theme.actFg.opacity(0.55) : Theme.tx2)
                VStack(alignment: .leading, spacing: 0) {
                    Text(clock(TimeInterval(durationMs) / 1000)).font(.system(size: 13, weight: .semibold).monospacedDigit())
                    Text(mode == 0 ? "Codec2 700C" : "Codec2 1200")
                        .font(.system(size: 11))
                        .opacity(0.7)
                }
            }
            .padding(.leading, 4)
            .padding(.trailing, 14)
            .padding(.vertical, 2)
            .background(message.outgoing ? Theme.actBg : Theme.s2, in: RoundedRectangle(cornerRadius: 18))
            .foregroundStyle(message.outgoing ? Theme.actFg : Theme.tx)
        }
    }
}

/// An image over LoRa: the preview or the full thumbnail, and how to get the full one.
private struct ImageCard: View {
    let message: StoredMessage
    @Environment(AppModel.self) private var model
    @State private var error: String?

    var body: some View {
        if case let .image(_, _, full, width, height, fullLen, data) = message.message {
            VStack(alignment: .leading, spacing: 0) {
                ZStack(alignment: .bottomLeading) {
                    if let ui = UIImage(data: data) {
                        Image(uiImage: ui)
                            .resizable()
                            .interpolation(.medium)
                            .aspectRatio(CGFloat(max(width, 1)) / CGFloat(max(height, 1)), contentMode: .fill)
                            .blur(radius: full ? 0 : 1.5)
                    } else {
                        Theme.s2
                    }
                    if !full {
                        Text("LoRa preview · \(data.count) bytes")
                            .font(.system(size: 11))
                            .padding(.horizontal, 8)
                            .padding(.vertical, 3)
                            .background(Theme.bg.opacity(0.85), in: RoundedRectangle(cornerRadius: 6))
                            .padding(8)
                    }
                }
                .frame(width: 250, height: 250 * CGFloat(max(height, 1)) / CGFloat(max(width, 1)))
                .clipped()
                VStack(alignment: .leading, spacing: 8) {
                    HStack {
                        Text("Photo").font(.system(size: 13, weight: .semibold))
                        Spacer()
                        Text(ByteCountFormatter.string(fromByteCount: Int64(full ? data.count : Int(fullLen)), countStyle: .file))
                            .font(.system(size: 13))
                            .foregroundStyle(Theme.tx2)
                    }
                    HStack(spacing: 6) {
                        TransportBadge(kind: .lora, label: full ? String(localized: "LoRa · full thumbnail") : String(localized: "LoRa · preview"))
                        if !full && !message.outgoing { Text("full one on request").font(.system(size: 11)).foregroundStyle(Theme.tx2) }
                    }
                    if !message.outgoing && !full && fullLen > 0 {
                        if model.requestedFull.contains(message.id) {
                            Label("Full image requested…", systemImage: "hourglass")
                                .font(.system(size: 13))
                                .foregroundStyle(Theme.tx2)
                        } else {
                            Button(fullLabel(Int(fullLen))) { error = model.requestFull(message) }
                                .buttonStyle(SecondaryButtonStyle(background: Theme.loraBg, foreground: Theme.loraFg, height: 40))
                                .disabled(!model.isConnected)
                        }
                    }
                }
                .padding(.horizontal, 12)
                .padding(.vertical, 10)
            }
            .frame(width: 250)
            .background(Theme.s1)
            .clipShape(RoundedRectangle(cornerRadius: 18))
            .overlay(RoundedRectangle(cornerRadius: 18).stroke(Theme.line))
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
        return String(localized: "Full image: \(fragmentsText(e.fragments)), ≈ \(String(format: "%.0f", Double(e.airtimeMs) / 1000)) s")
    }
}

private struct Composer: View {
    let chat: ChatID
    @Binding var draft: String
    @Binding var error: String?
    @Environment(AppModel.self) private var model
    @State private var picked: PhotosPickerItem?
    @State private var picking = false
    @State private var image: PreparedImage?
    @State private var preparing = false

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            if let line = costLine {
                Text(line).font(.system(size: 11)).foregroundStyle(Theme.tx2).padding(.leading, 52)
            }
            if model.recorder.recording {
                recordingBar
            } else {
                HStack(spacing: 8) {
                    if isContact {
                        Button { picking = true } label: {
                            Image(systemName: preparing ? "hourglass" : "plus")
                                .font(.system(size: 18, weight: .semibold))
                                .frame(width: 44, height: 44)
                                .background(Theme.s2, in: Circle())
                                .foregroundStyle(Theme.tx)
                        }
                        .disabled(!model.isConnected || preparing)
                        .accessibilityLabel("Attach a photo")
                    }
                    TextField("Message", text: $draft, axis: .vertical)
                        .lineLimit(1...5)
                        .font(.system(size: 16))
                        .padding(.horizontal, 14)
                        .padding(.vertical, 9)
                        .background(Theme.bg, in: RoundedRectangle(cornerRadius: 22))
                        .overlay(RoundedRectangle(cornerRadius: 22).stroke(Theme.line))
                    if isContact && draft.isEmpty {
                        Button {
                            Task { await model.recorder.start() }
                        } label: {
                            Image(systemName: "mic")
                                .font(.system(size: 20, weight: .semibold))
                                .frame(width: 44, height: 44)
                                .foregroundStyle(Theme.lora)
                        }
                        .disabled(!model.isConnected)
                        .accessibilityLabel("Record a voice message")
                    } else {
                        Button {
                            let text = draft.trimmingCharacters(in: .whitespacesAndNewlines)
                            if let e = model.send(text, to: chat) { error = e } else { draft = "" }
                        } label: {
                            Image(systemName: "arrow.up")
                                .font(.system(size: 18, weight: .bold))
                                .frame(width: 44, height: 44)
                                .background(Theme.actBg, in: Circle())
                                .foregroundStyle(Theme.actFg)
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
        .background(Theme.s1)
        .overlay(alignment: .top) { Rectangle().fill(Theme.line).frame(height: 1) }
        .photosPicker(isPresented: $picking, selection: $picked, matching: .images)
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
        .onAppear {
            #if DEBUG
            // -screen media: the photo sheet with the demo landscape.
            if model.debugScreen == "media", isContact, image == nil,
               let ui = UIImage(named: "AppIcon") ?? Self.sampleImage(), let (rgba, w, h) = ui.rgba() {
                image = try? prepareImage(rgba: rgba, width: w, height: h)
            }
            #endif
        }
        .sheet(item: Binding(get: { image.map(ImageDraft.init) }, set: { image = $0?.image })) { draft in
            SendPhotoSheet(chat: chat, image: draft.image) { e in
                image = nil
                error = e
            }
        }
        .alert("Microphone access is off", isPresented: Binding(get: { model.recorder.denied }, set: { model.recorder.denied = $0 })) {
            Button("OK", role: .cancel) {}
        } message: {
            Text("Allow it in Settings to record voice messages.")
        }
    }

    /// A drawn mountain scene for the debug photo sheet.
    static func sampleImage() -> UIImage? {
        let r = UIGraphicsImageRenderer(size: CGSize(width: 320, height: 240))
        return r.image { ctx in
            UIColor(red: 0.25, green: 0.3, blue: 0.55, alpha: 1).setFill()
            ctx.fill(CGRect(x: 0, y: 0, width: 320, height: 240))
            UIColor(white: 0.35, alpha: 1).setFill()
            let p = UIBezierPath()
            p.move(to: CGPoint(x: 0, y: 200)); p.addLine(to: CGPoint(x: 120, y: 70)); p.addLine(to: CGPoint(x: 200, y: 140))
            p.addLine(to: CGPoint(x: 260, y: 90)); p.addLine(to: CGPoint(x: 320, y: 160)); p.addLine(to: CGPoint(x: 320, y: 240))
            p.addLine(to: CGPoint(x: 0, y: 240)); p.close(); p.fill()
            UIColor(red: 0.15, green: 0.3, blue: 0.18, alpha: 1).setFill()
            ctx.fill(CGRect(x: 0, y: 190, width: 320, height: 50))
        }
    }

    private var isContact: Bool {
        if case .contact = chat { return true }
        return false
    }

    /// While recording: time, cost so far, cancel, send.
    private var recordingBar: some View {
        let seconds = model.recorder.elapsed
        // 700C: 87.5 bytes a second, plus the 8-byte header.
        let estimate = model.estimate(chat, datagramLen: 8 + UInt32(seconds * 87.5))
        return HStack(spacing: 12) {
            Button {
                _ = model.recorder.stop(keep: false)
            } label: {
                Image(systemName: "xmark")
                    .font(.system(size: 16, weight: .bold))
                    .frame(width: 44, height: 44)
                    .background(Theme.s2, in: Circle())
                    .foregroundStyle(Theme.tx)
            }
            .accessibilityLabel("Cancel")
            Circle().fill(Theme.sos).frame(width: 10, height: 10)
            VStack(alignment: .leading, spacing: 1) {
                Text("\(clock(seconds)) / \(clock(VoiceRecorder.maxSeconds))")
                    .font(.system(size: 15, weight: .semibold).monospacedDigit())
                if let e = estimate {
                    Text("\(fragmentsText(e.fragments)) · ≈ \(String(format: "%.0f", Double(e.airtimeMs) / 1000)) s of airtime")
                        .font(.system(size: 11))
                        .foregroundStyle(Theme.tx2)
                }
            }
            Spacer()
            Button {
                guard let pcm = model.recorder.stop(keep: true) else { return }
                let voice = prepareVoice(pcm: pcm)
                if let e = model.sendVoice(voice, to: chat) { error = e }
            } label: {
                Image(systemName: "arrow.up")
                    .font(.system(size: 18, weight: .bold))
                    .frame(width: 44, height: 44)
                    .background(Theme.actBg, in: Circle())
                    .foregroundStyle(Theme.actFg)
            }
            .accessibilityLabel("Send")
        }
        .frame(minHeight: 44)
    }

    private var costLine: String? {
        if case .group = chat { return String(localized: "Flooded to every member, without acknowledgement") }
        guard let e = model.estimate(chat, text: draft) else { return nil }
        let s = String(format: "%.1f", Double(e.airtimeMs) / 1000)
        return String(localized: "Estimated cost: \(fragmentsText(e.fragments)) · ≈ \(s) s of airtime")
    }
}

private struct ImageDraft: Identifiable {
    let image: PreparedImage
    var id: Int { image.full.hashValue }
}

/// How to send a photo, and what each way costs (mockup `media`).
private struct SendPhotoSheet: View {
    let chat: ChatID
    let image: PreparedImage
    let done: (String?) -> Void
    @Environment(AppModel.self) private var model

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 12) {
                    Theme.s2
                        .frame(maxWidth: .infinity)
                        .frame(height: 230)
                        .overlay {
                            if let ui = UIImage(data: image.full) {
                                Image(uiImage: ui).resizable().interpolation(.medium).scaledToFill()
                            }
                        }
                        .clipShape(RoundedRectangle(cornerRadius: 16))
                        .overlay(alignment: .bottomLeading) {
                            Text("\(Int(image.width))×\(Int(image.height)) AVIF · preview \(image.preview.count) B · full \(image.full.count) B")
                                .font(.system(size: 12))
                                .padding(.horizontal, 8)
                                .padding(.vertical, 4)
                                .background(Theme.bg, in: RoundedRectangle(cornerRadius: 6))
                                .padding(12)
                        }
                    SectionLabel("How to send it to \(model.title(of: chat))")
                    option(
                        title: String(localized: "LoRa preview"),
                        detail: String(localized: "160×120 AVIF · \(previewFragments) · direct only; the full thumbnail if they ask"),
                        badge: costBadge, kind: .lora, selected: true, enabled: true
                    )
                    option(title: String(localized: "Full resolution via iroh"),
                           detail: String(localized: "1-packet LoRa announcement, peer-to-peer transfer"),
                           badge: String(localized: "1 packet"), kind: .iroh, selected: false, enabled: false)
                    option(title: String(localized: "Full resolution via Tor"),
                           detail: String(localized: "Anonymous, slower"),
                           badge: String(localized: "1 packet"), kind: .tor, selected: false, enabled: false)
                    option(title: String(localized: "Thumbnail now, full resolution later"),
                           detail: String(localized: "Sent when the Internet comes back"),
                           badge: costBadge, kind: .lora, selected: false, enabled: false)
                    LaterNote(text: "iroh, Tor and later sending come with the Internet bridge (L7).")
                    Text("Airtime left: \(seconds(model.airtimeLeftMs)) s. Images are never flooded.")
                        .font(.system(size: 13))
                        .foregroundStyle(Theme.tx2)
                    Button("Send the preview") { done(model.sendImage(image, to: chat)) }
                        .buttonStyle(.primary)
                        .padding(.top, 8)
                }
                .padding(16)
            }
            .screen()
            .navigationTitle("Send a photo")
            .navigationBarTitleDisplayMode(.inline)
            .toolbarBackground(Theme.s1, for: .navigationBar)
            .toolbarBackground(.visible, for: .navigationBar)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { done(nil) } }
            }
        }
    }

    private var estimate: Estimate? { model.estimate(chat, datagramLen: imageDatagramLen(image: image)) }

    private var previewFragments: String {
        estimate.map { fragmentsText($0.fragments) } ?? fragmentsText(3)
    }

    private var costBadge: String {
        guard let e = estimate else { return "LoRa" }
        return String(localized: "≈ \(String(format: "%.0f", max(1, Double(e.airtimeMs) / 1000))) s")
    }

    private func option(title: String, detail: String, badge: String, kind: TransportBadge.Kind,
                        selected: Bool, enabled: Bool) -> some View {
        HStack(spacing: 12) {
            Circle()
                .strokeBorder(selected ? Theme.tx : Theme.line, lineWidth: 2)
                .frame(width: 22, height: 22)
                .overlay { if selected { Circle().fill(Theme.tx).frame(width: 10, height: 10) } }
            VStack(alignment: .leading, spacing: 3) {
                Text(title).font(.system(size: 15, weight: .semibold))
                Text(detail).font(.system(size: 13)).foregroundStyle(Theme.tx2)
            }
            Spacer(minLength: 4)
            TransportBadge(kind: kind, label: badge)
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 12)
        .background(selected ? Theme.s2 : Theme.s1, in: RoundedRectangle(cornerRadius: 12))
        .opacity(enabled ? 1 : 0.5)
        .accessibilityElement(children: .combine)
        .accessibilityAddTraits(selected ? .isSelected : [])
    }
}
