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
    @State private var renaming = false

    var body: some View {
        VStack(spacing: 0) {
            ScrollViewReader { proxy in
                ScrollView {
                    LazyVStack(spacing: 10) {
                        sessionLine
                        ForEach(Array(messages.enumerated()), id: \.element.id) { i, m in
                            // A separator when the day changes: "Today", "Yesterday", the date.
                            if i == 0 || !Calendar.current.isDate(messages[i - 1].date, inSameDayAs: m.date) {
                                DaySeparator(date: m.date)
                            }
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
                // Opens on the latest messages; later ones scroll into view.
                .defaultScrollAnchor(.bottom)
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
                // A contact's name: tap to rename it on this phone.
                VStack(spacing: 2) {
                    Text(model.title(of: chat)).font(.system(size: 17, weight: .semibold))
                    Text(subtitle).font(.system(size: 12)).foregroundStyle(Theme.tx2)
                }
                .onTapGesture { if !isGroup { renaming = true } }
                .accessibilityAddTraits(isGroup ? [] : .isButton)
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
            // On screen: its messages need no notification.
            model.notifier.visibleChat = chat
            model.notifier.clear(chat)
            #if DEBUG
            if model.debugScreen == "group", isGroup { showingGroup = true }
            #endif
        }
        .onDisappear {
            if model.notifier.visibleChat == chat { model.notifier.visibleChat = nil }
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
        .renameAlert(node: chatKey, isPresented: $renaming)
    }

    private var chatKey: Data? {
        if case let .contact(key) = chat { return key }
        return nil
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
            let c = model.contacts.first { $0.key == key }
            // Renamed: the node's own name first.
            let hops = [c.flatMap { $0.nodeName != $0.name ? $0.nodeName : nil }, hopsBadge(c?.hops)]
                .compactMap { $0 }.joined(separator: " · ")
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

/// "Today", "Yesterday", or the date, between the messages of two days.
struct DaySeparator: View {
    let date: Date

    var body: some View {
        Text(label)
            .font(.system(size: 12))
            .foregroundStyle(Theme.tx2)
            .padding(.horizontal, 10)
            .padding(.vertical, 3)
            .background(Theme.s2, in: RoundedRectangle(cornerRadius: 10))
            .frame(maxWidth: .infinity)
            .padding(.vertical, 4)
    }

    private var label: String {
        let cal = Calendar.current
        if cal.isDateInToday(date) { return String(localized: "Today") }
        if cal.isDateInYesterday(date) { return String(localized: "Yesterday") }
        return date.formatted(.dateTime.weekday(.wide).day().month(.wide))
    }
}

extension View {
    /// Names a contact on this phone only (a nickname in the app's database): never sent; the
    /// radio's contact list and the other phones keep their names. Blank: the node's name.
    func renameAlert(node: Data?, isPresented: Binding<Bool>) -> some View {
        modifier(RenameAlert(node: node, isPresented: isPresented))
    }
}

private struct RenameAlert: ViewModifier {
    let node: Data?
    @Binding var isPresented: Bool
    @Environment(AppModel.self) private var model
    @State private var name = ""

    func body(content: Content) -> some View {
        let c = node.flatMap { model.contact($0) }
        content
            .alert("Rename", isPresented: $isPresented) {
                TextField(c?.nodeName ?? "", text: $name)
                Button("Save") { if let node { model.renameContact(node, to: name) } }
                if let c, c.name != c.nodeName {
                    Button("Use \(c.nodeName) again") { if let node { model.renameContact(node, to: nil) } }
                }
                Button("Cancel", role: .cancel) {}
            } message: {
                Text("Only on this phone: their radio calls itself \(c?.nodeName ?? ""), and they are not told.")
            }
            .onChange(of: isPresented) {
                if isPresented, let c { name = c.name != c.nodeName ? c.name : "" }
            }
    }
}

/// "Codec2 1600" for a VOICE mode (§13.1).
func codecName(_ mode: UInt8) -> String {
    switch mode {
    case 0: "Codec2 700C"
    case 1: "Codec2 1200"
    case 2: "Codec2 1600"
    case 3: "Codec2 2400"
    default: "Codec2"
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
            case let .announce(_, mediaId, _, _, _, _, _):
                // Shown inside its LoRa preview's card when that preview is here.
                if model.preview(for: mediaId, in: message.chat) == nil {
                    BridgeSection(announce: message, standalone: true)
                }
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
            // When it was sent (ours) or received, then how it went.
            let time = message.date.formatted(date: .omitted, time: .shortened)
            Text(message.outgoing ? ([time] + [caption].compactMap { $0 }).joined(separator: " · ") : time)
                .font(.system(size: 11))
                .foregroundStyle(Theme.tx2)
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
                    Text(codecName(mode))
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
        if case let .image(_, mediaId, full, width, height, fullLen, data) = message.message {
            let announce = model.announcement(for: mediaId, in: message.chat)
            let fetched = announce.flatMap { a -> UIImage? in
                guard case let .announce(_, _, _, hash, _, _, _) = a.message,
                      let url = model.downloads[hash] else { return nil }
                return UIImage(contentsOfFile: url.path)
            }
            VStack(alignment: .leading, spacing: 0) {
                ZStack(alignment: .bottomLeading) {
                    if let fetched {
                        Image(uiImage: fetched)
                            .resizable()
                            .aspectRatio(CGFloat(max(width, 1)) / CGFloat(max(height, 1)), contentMode: .fill)
                    } else if let ui = UIImage(data: data) {
                        Image(uiImage: ui)
                            .resizable()
                            .interpolation(.medium)
                            .aspectRatio(CGFloat(max(width, 1)) / CGFloat(max(height, 1)), contentMode: .fill)
                            .blur(radius: full ? 0 : 1.5)
                    } else {
                        Theme.s2
                    }
                    if !full && fetched == nil {
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
                    if let announce {
                        BridgeSection(announce: announce, standalone: false)
                    } else if !message.outgoing && !full && fullLen > 0 {
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
    @State private var original: Data?
    @State private var preparing = false
    @State private var locating = false

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            if let share = model.share(of: chat) {
                Button { locating = true } label: {
                    Label(sharingText(share), systemImage: "location.fill")
                        .font(.system(size: 12, weight: .semibold))
                        .foregroundStyle(Theme.tx2)
                }
                .padding(.leading, 52)
            }
            if let line = costLine {
                Text(line).font(.system(size: 11)).foregroundStyle(Theme.tx2).padding(.leading, 52)
            }
            if model.recorder.recording {
                recordingBar
            } else {
                HStack(spacing: 8) {
                    Menu {
                        if isContact || !model.bridgeProviders(anonymous: false).isEmpty {
                            Button { picking = true } label: { Label("Photo", systemImage: "photo") }
                                .disabled(!model.isConnected)
                        }
                        Button { locating = true } label: {
                            Label("Share my location", systemImage: "location")
                        }
                    } label: {
                        Image(systemName: preparing ? "hourglass" : "plus")
                            .font(.system(size: 18, weight: .semibold))
                            .frame(width: 44, height: 44)
                            .background(Theme.s2, in: Circle())
                            .foregroundStyle(Theme.tx)
                    }
                    .disabled(preparing)
                    .accessibilityLabel("Attach")
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
                original = data
                do {
                    image = try await Task.detached { try prepareImage(rgba: rgba, width: w, height: h) }.value
                } catch {
                    self.error = LichenError.BadImage.display
                }
            }
        }
        .onAppear {
            #if DEBUG
            // -screen share: the location sheet.
            if model.debugScreen == "share" { locating = true }
            // -screen media: the photo sheet with the demo landscape.
            if model.debugScreen == "media", isContact, image == nil,
               let ui = UIImage(named: "AppIcon") ?? Self.sampleImage(), let (rgba, w, h) = ui.rgba() {
                image = try? prepareImage(rgba: rgba, width: w, height: h)
            }
            #endif
        }
        .sheet(isPresented: $locating) { ChatShareSheet(chat: chat) }
        .sheet(item: Binding(get: { image.map(ImageDraft.init) }, set: { image = $0?.image })) { draft in
            SendPhotoSheet(chat: chat, image: draft.image, original: original) { e in
                image = nil
                original = nil
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
        // The chosen mode's bytes for what is recorded so far.
        let estimate = model.estimate(chat, datagramLen: voiceDatagramLenFor(mode: UInt8(model.voiceMode), durationMs: UInt32(seconds * 1000)))
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
                let voice = prepareVoice(pcm: pcm, mode: UInt8(model.voiceMode))
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

    private func sharingText(_ share: ShareInfo) -> String {
        let until = Date(timeIntervalSince1970: TimeInterval(share.untilS)).formatted(date: .omitted, time: .shortened)
        return String(localized: "Sharing your location (\(PrecisionText.name(share.precision))) until \(until)")
    }

    private var costLine: String? {
        if case .group = chat { return String(localized: "Flooded to every member, without acknowledgement") }
        guard let e = model.estimate(chat, text: draft) else { return nil }
        let s = String(format: "%.1f", Double(e.airtimeMs) / 1000)
        return String(localized: "Estimated cost: \(fragmentsText(e.fragments)) · ≈ \(s) s of airtime")
    }
}

/// Our position for this chat, from the composer's menu: the same shares as the map's panel.
private struct ChatShareSheet: View {
    let chat: ChatID
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    @State private var precision: SharePrecision = .exact
    @State private var hours = 1.0
    @State private var error: String?

    var body: some View {
        let current = model.share(of: chat)
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 12) {
                    SectionLabel("Precision")
                    Picker("Precision", selection: $precision) {
                        ForEach([SharePrecision.exact, .m100, .km1, .km10], id: \.self) { p in
                            Text(PrecisionText.name(p)).tag(p)
                        }
                    }
                    .pickerStyle(.segmented)
                    Text("A rounded position is the centre of a fixed grid cell, never random noise: averaging many of them reveals nothing more.")
                        .font(.system(size: 12)).foregroundStyle(Theme.tx2)
                    SectionLabel("Duration").padding(.top, 8)
                    Picker("Duration", selection: $hours) {
                        Text("1 h").tag(1.0)
                        Text("8 h").tag(8.0)
                        Text("24 h").tag(24.0)
                    }
                    .pickerStyle(.segmented)
                    Text("Sharing stops on its own at the end; the others see your marker disappear.")
                        .font(.system(size: 12)).foregroundStyle(Theme.tx2)
                    if case .group = chat {
                        Text("A group gets at most one position every 5 minutes.")
                            .font(.system(size: 12)).foregroundStyle(Theme.tx2)
                    }
                    Button(current == nil ? "Share" : "Update") { apply(precision) }
                        .buttonStyle(.primary)
                        .padding(.top, 8)
                    if current != nil {
                        Button("Stop sharing") { apply(nil) }
                            .buttonStyle(.destructive)
                    }
                }
                .padding(16)
            }
            .screen()
            .navigationTitle("Share my location")
            .navigationBarTitleDisplayMode(.inline)
            .toolbarBackground(Theme.s1, for: .navigationBar)
            .toolbarBackground(.visible, for: .navigationBar)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }
                }
            }
            .alert("Could not share", isPresented: Binding(get: { error != nil }, set: { if !$0 { error = nil } })) {
                Button("OK") { error = nil }
            } message: {
                Text(error ?? "")
            }
        }
        .presentationDetents([.medium, .large])
        .onAppear {
            if let p = current?.precision { precision = p }
        }
    }

    private func apply(_ p: SharePrecision?) {
        if let e = model.setShare(chat, precision: p, hours: hours) {
            error = e
        } else {
            dismiss()
        }
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
    /// The photo as picked, offered over the bridge in full resolution.
    let original: Data?
    let done: (String?) -> Void
    @Environment(AppModel.self) private var model
    @State private var mode: Mode?

    enum Mode { case lora, iroh, tor }

    private var isGroup: Bool {
        if case .group = chat { return true }
        return false
    }

    private var irohOK: Bool { model.bridgeOnline && original != nil }
    private var torOK: Bool { model.torStatus == .ready && original != nil }

    /// LoRa previews go to contacts only; groups get the announcement (§14.1).
    private var chosen: Mode {
        if let mode { return mode }
        if !isGroup { return .lora }
        return irohOK || !torOK ? .iroh : .tor
    }

    private var bridgeChosen: Bool { chosen != .lora }
    private var sendable: Bool {
        switch chosen {
        case .lora: !isGroup
        case .iroh: irohOK
        case .tor: torOK
        }
    }

    private var size: String {
        ByteCountFormatter.string(fromByteCount: Int64(original?.count ?? 0), countStyle: .file)
    }

    private var irohDetail: String {
        if irohOK { return String(localized: "1-packet LoRa announcement, peer-to-peer transfer (\(size))") }
        if !model.irohEnabled { return String(localized: "iroh is off in Settings") }
        return String(localized: "Needs an Internet connection")
    }

    private var torDetail: String {
        switch model.torStatus {
        case .ready: return String(localized: "Anonymous, slower: they fetch it from your onion service (\(size))")
        case .connecting, .publishing: return String(localized: "Connecting to Tor…")
        case .failed: return String(localized: "Tor could not connect")
        case .off: return String(localized: "Turn Tor on in Settings")
        }
    }

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
                        detail: isGroup
                            ? String(localized: "Contacts only: never flooded to a group")
                            : String(localized: "160×120 AVIF · \(previewFragments) · direct only; the full thumbnail if they ask"),
                        badge: costBadge, kind: .lora, selected: chosen == .lora, enabled: !isGroup
                    )
                    .onTapGesture { if !isGroup { mode = .lora } }
                    option(title: String(localized: "Full resolution via iroh"), detail: irohDetail,
                           badge: String(localized: "1 packet"), kind: .iroh, selected: chosen == .iroh, enabled: irohOK)
                    .onTapGesture { if irohOK { mode = .iroh } }
                    option(title: String(localized: "Full resolution via Tor"), detail: torDetail,
                           badge: String(localized: "1 packet"), kind: .tor, selected: chosen == .tor, enabled: torOK)
                    .onTapGesture { if torOK { mode = .tor } }
                    option(title: String(localized: "Thumbnail now, full resolution later"),
                           detail: String(localized: "Sent when the Internet comes back"),
                           badge: costBadge, kind: .lora, selected: false, enabled: false)
                    LaterNote(text: "Sending later comes in a later version.")
                    Text("Airtime left: \(seconds(model.airtimeLeftMs)) s. Images are never flooded.")
                        .font(.system(size: 13))
                        .foregroundStyle(Theme.tx2)
                    Button(bridgeChosen ? String(localized: "Send the announcement") : String(localized: "Send the preview")) {
                        if bridgeChosen, let original {
                            done(model.sendOverBridge(original, name: Self.name(of: original), to: chat,
                                                      anonymous: chosen == .tor))
                        } else {
                            done(model.sendImage(image, to: chat))
                        }
                    }
                    .buttonStyle(.primary)
                    .disabled(!sendable)
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

    /// A file name from the image's format.
    static func name(of data: Data) -> String {
        let head = [UInt8](data.prefix(12))
        let ext: String
        if head.starts(with: [0xFF, 0xD8]) { ext = "jpg" }
        else if head.starts(with: [0x89, 0x50, 0x4E, 0x47]) { ext = "png" }
        else if head.count >= 12, String(bytes: head[4..<12], encoding: .ascii)?.hasPrefix("ftyphei") == true { ext = "heic" }
        else if head.count >= 12, String(bytes: head[4..<12], encoding: .ascii) == "ftypavif" { ext = "avif" }
        else { ext = "jpg" }
        let stamp = Date().formatted(.iso8601.year().month().day().time(includingFractionalSeconds: false))
            .replacingOccurrences(of: ":", with: "")
        return "photo-\(stamp).\(ext)"
    }

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

/// Content announced over LoRa and fetched over the Internet bridge (§14, mockup `chat`):
/// size, transports, the download, "hash verified" once done.
private struct BridgeSection: View {
    let announce: StoredMessage
    /// Its own card (no LoRa preview in this conversation).
    let standalone: Bool
    @Environment(AppModel.self) private var model

    var body: some View {
        if case let .announce(_, _, _, hash, size, providers, name) = announce.message {
            let file = model.downloads[hash]
            let content = VStack(alignment: .leading, spacing: 8) {
                if standalone, let file, let ui = UIImage(contentsOfFile: file.path) {
                    Image(uiImage: ui).resizable().scaledToFit()
                        .clipShape(RoundedRectangle(cornerRadius: 12))
                }
                HStack {
                    Text(verbatim: name).font(.system(size: 13, weight: .semibold)).lineLimit(1)
                    Spacer()
                    Text(ByteCountFormatter.string(fromByteCount: Int64(size), countStyle: .file))
                        .font(.system(size: 13)).foregroundStyle(Theme.tx2)
                }
                HStack(spacing: 6) {
                    ForEach(providers, id: \.transport) { p in
                        switch p.transport {
                        case 1: TransportBadge(kind: .iroh, label: "iroh")
                        case 2: TransportBadge(kind: .tor, label: "Tor")
                        default: EmptyView()
                        }
                    }
                    if file != nil {
                        Label("hash verified", systemImage: "checkmark.seal")
                            .font(.system(size: 11))
                            .foregroundStyle(Theme.ok)
                    }
                }
                if let error = model.fetchErrors[hash] {
                    Text(error).font(.system(size: 12)).foregroundStyle(Theme.sosFg)
                }
                if file == nil && !announce.outgoing {
                    if let via = model.fetching[hash] {
                        HStack(spacing: 8) {
                            ProgressView()
                            Text(via == 2 ? String(localized: "Downloading over Tor: it can take a minute…")
                                          : String(localized: "Downloading over iroh…"))
                                .font(.system(size: 13)).foregroundStyle(Theme.tx2)
                        }
                    } else {
                        let route = model.fetchRoute(providers)
                        let torOnly = route.first?.transport == 2
                        Button("Download in full resolution") { model.fetch(announce) }
                            .buttonStyle(SecondaryButtonStyle(background: torOnly ? Theme.torBg : Theme.irohBg,
                                                              foreground: torOnly ? Theme.torFg : Theme.irohFg, height: 40))
                            .disabled(route.isEmpty)
                    }
                }
            }
            if standalone {
                content
                    .padding(12)
                    .frame(width: 250)
                    .background(Theme.s1)
                    .clipShape(RoundedRectangle(cornerRadius: 18))
                    .overlay(RoundedRectangle(cornerRadius: 18).stroke(Theme.line))
            } else {
                content
            }
        }
    }
}

