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
    @Environment(\.dismiss) private var dismissChat
    /// The bottom-most message on screen: new ones follow only when it is the last one.
    @State private var bottomVisible: Int64?
    /// Messages that arrived while reading above.
    @State private var unseen = 0
    /// The message to keep at the top while an earlier page comes in.
    @State private var keepAt: Int64?
    /// The first message that was unread when the chat opened, and how many were: the divider
    /// stays there until the chat is left.
    @State private var dividerBefore: Int64?
    @State private var unreadAtOpen = 0
    /// A message opened from a search, highlighted for a moment.
    @State private var highlight: Int64?
    /// The message being answered (§6.2).
    @State private var replyTo: StoredMessage?
    /// A quoted original to go to.
    @State private var jumpTo: Int64?

    var body: some View {
        VStack(spacing: 0) {
            ScrollViewReader { proxy in
                ScrollView {
                    // Not lazy: pages of 100 messages, and exact heights let the view open on the
                    // last one (a lazy stack estimates photos and voice notes, and lands short).
                    VStack(spacing: 10) {
                        if let e = model.historyError[chat] {
                            Banner(kind: .warn, title: e, action: (String(localized: "Retry"), { model.load(chat) }))
                        }
                        if model.olderAvailable[chat] == true {
                            Button("Show earlier messages") { showEarlier() }
                            .font(.system(.subheadline, weight: .semibold))
                            .frame(minHeight: 44)
                        }
                        sessionLine
                        ForEach(Array(messages.enumerated()), id: \.element.id) { i, m in
                            // A separator when the day changes: "Today", "Yesterday", the date.
                            if i == 0 || !Calendar.current.isDate(messages[i - 1].date, inSameDayAs: m.date) {
                                DaySeparator(date: m.date)
                            }
                            if m.id == dividerBefore {
                                UnreadDivider(count: unreadAtOpen)
                            }
                            MessageView(message: m, isGroup: isGroup, error: $error, highlighted: highlight == m.id,
                                        reply: { replyTo = m }, openQuote: { jumpTo = $0 })
                                .id(m.id)
                        }
                        if let failed = lastFailure {
                            FailureBanner(message: failed) { error = model.retry(failed) }
                                .padding(.top, 4)
                        }
                        Color.clear.frame(height: 1).id(Self.bottom)
                    }
                    .scrollTargetLayout()
                    .padding(.horizontal, 14)
                    .padding(.vertical, 12)
                }
                .scrollPosition(id: $bottomVisible, anchor: .bottom)
                .defaultScrollAnchor(.bottom)
                .overlay(alignment: .bottom) {
                    if unseen > 0 {
                        Button {
                            withAnimation { proxy.scrollTo(Self.bottom, anchor: .bottom) }
                            unseen = 0
                        } label: {
                            Label(String(localized: "\(unseen) new messages"), systemImage: "arrow.down")
                                .font(.system(.subheadline, weight: .semibold))
                                .padding(.horizontal, 16)
                                .frame(minHeight: 44)
                                .background(Theme.actBg, in: Capsule())
                                .foregroundStyle(Theme.actFg)
                        }
                        .padding(.bottom, 10)
                    }
                }
                // Opens on the latest messages, once the history is loaded (onAppear) and again
                // once images are laid out; later ones scroll into view.
                // On a message from a search, or on the unread divider, else at the end.
                .task(id: chat) {
                    await Task.yield()
                    scrollToStart(proxy)
                    try? await Task.sleep(for: .milliseconds(250))
                    scrollToStart(proxy)
                    if highlight != nil {
                        try? await Task.sleep(for: .seconds(2))
                        withAnimation { highlight = nil }
                    }
                }
                // A new last message: follow it when we were at the bottom or sent it; else count
                // it in the pill. Older pages, loaded at the top, move nothing.
                .onChange(of: messages.last?.id) { old, new in
                    guard let new, new != old else { return }
                    let wasAtBottom = bottomVisible == nil || bottomVisible == old
                    if wasAtBottom || messages.last?.outgoing == true {
                        withAnimation { proxy.scrollTo(Self.bottom, anchor: .bottom) }
                        unseen = 0
                    } else {
                        unseen += 1
                    }
                    model.markRead(chat)
                }
                // Stay on the message that was at the top, once the earlier page is laid out.
                .onChange(of: messages.first?.id) {
                    if let keep = keepAt {
                        proxy.scrollTo(keep, anchor: .top)
                        keepAt = nil
                    }
                }
                #if DEBUG
                // -earlier YES: the earlier page at once, as the button does (paging checks).
                .task(id: chat) {
                    guard UserDefaults.standard.bool(forKey: "earlier") else { return }
                    try? await Task.sleep(for: .seconds(2))
                    showEarlier()
                }
                #endif
                .onChange(of: bottomVisible) {
                    if bottomVisible == messages.last?.id { unseen = 0 }
                }
                // A quote tapped: its original, loading earlier pages if needed.
                .onChange(of: jumpTo) {
                    guard let id = jumpTo else { return }
                    jumpTo = nil
                    model.loadUntil(chat, id)
                    highlight = id
                    Task {
                        try? await Task.sleep(for: .milliseconds(100))
                        withAnimation { proxy.scrollTo(id, anchor: .center) }
                        try? await Task.sleep(for: .seconds(2))
                        withAnimation { if highlight == id { highlight = nil } }
                    }
                }
            }
            Composer(chat: chat, draft: $draft, error: $error, replyTo: $replyTo)
        }
        .screen()
        .navigationBarTitleDisplayMode(.inline)
        .toolbarBackground(Theme.s1, for: .navigationBar)
        .toolbarBackground(.visible, for: .navigationBar)
        .toolbar {
            ToolbarItem(placement: .principal) {
                // A contact's name: tap to rename it on this phone.
                VStack(spacing: 2) {
                    Text(model.title(of: chat)).font(.system(.body, weight: .semibold))
                    Text(subtitle).font(.system(.caption)).foregroundStyle(Theme.tx2)
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
            if let f = model.focus, f.chat == chat {
                model.focus = nil
                model.loadUntil(chat, f.id)
                highlight = f.id
            }
            // Before marking them read: where the unread messages start.
            let unread = messages.filter { !$0.outgoing && !$0.read }
            dividerBefore = unread.first?.id
            unreadAtOpen = unread.count
            model.markRead(chat)
            // On screen: its messages need no notification.
            model.notifier.visibleChat = chat
            model.notifier.clear(chat)
            #if DEBUG
            if model.debugScreen == "group", isGroup { showingGroup = true }
            #endif
        }
        .onAppear { if draft.isEmpty { draft = model.draft(chat) } }
        // A group we left, closed or were removed from: back to the conversations.
        .onChange(of: model.groups.map(\.gid)) { _, gids in
            if case let .group(gid) = chat, !gids.contains(gid) { dismissChat() }
        }
        .onChange(of: draft) { model.setDraft(chat, draft) }
        .onDisappear {
            if model.notifier.visibleChat == chat { model.notifier.visibleChat = nil }
            // A recording belongs to its conversation: leaving it drops the recording.
            model.recorder.cancel(for: chat)
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

    private func showEarlier() {
        keepAt = messages.first?.id
        model.loadOlder(chat)
    }

    /// The end of the conversation, to scroll to.
    private static let bottom = "bottom"

    /// Opens on a message from a search, on the first unread one, else at the end. Not at the
    /// end, the reading position is that message: a new one then shows the pill.
    private func scrollToStart(_ proxy: ScrollViewProxy) {
        if let h = highlight {
            proxy.scrollTo(h, anchor: .center)
            bottomVisible = h
        } else if let d = dividerBefore {
            // The first unread message near the top, its divider just above it.
            proxy.scrollTo(d, anchor: UnitPoint(x: 0.5, y: 0.12))
            bottomVisible = d
        } else {
            proxy.scrollTo(Self.bottom, anchor: .bottom)
        }
    }

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
                        .font(.system(.caption))
                        .foregroundStyle(c.verified ? Theme.tx2 : Theme.warnFg)
                        .labelStyle(TintedIconLabel(tint: c.verified ? Theme.ok : Theme.warn))
                }
                .buttonStyle(.plain)
                .padding(.top, 4)
            } else if model.secured.contains(key) {
                Label("Encrypted session", systemImage: "lock")
                    .font(.system(.caption))
                    .foregroundStyle(Theme.tx2)
                    .labelStyle(TintedIconLabel(tint: Theme.ok))
                    .padding(.top, 4)
            }
        } else if isGroup {
            Label("Signed group messages, flooded", systemImage: "lock")
                .font(.system(.caption))
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
            .font(.system(.caption))
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
            case .expired:
                Banner(kind: .warn, title: String(localized: "Expired"),
                       message: String(localized: "Not delivered within 24 hours, so it will not go out later. If part of it went out, it may have arrived."),
                       action: action)
            case .identityChanged:
                Banner(kind: .sos, title: String(localized: "Not sent: identity changed"),
                       message: String(localized: "Compare safety numbers with \(model.title(of: message.chat)) before accepting their new key."),
                       action: nil)
            }
        }
    }
}

/// "3 unread messages", before the first one that was unread when the chat opened.
private struct UnreadDivider: View {
    let count: Int

    var body: some View {
        HStack(spacing: 10) {
            Rectangle().fill(Theme.line).frame(height: 1)
            Text("\(count) unread messages")
                .font(.system(.caption, weight: .semibold))
                .foregroundStyle(Theme.tx2)
                .fixedSize()
            Rectangle().fill(Theme.line).frame(height: 1)
        }
        .padding(.vertical, 4)
        .accessibilityElement(children: .combine)
    }
}

/// What a reply answers (§6.2): its author and the start of it, or that it is not here.
private struct QuoteBox: View {
    let quote: Quote
    let chat: ChatID
    @Environment(AppModel.self) private var model

    var body: some View {
        HStack(spacing: 8) {
            RoundedRectangle(cornerRadius: 2).fill(Theme.tx2).frame(width: 3)
            VStack(alignment: .leading, spacing: 1) {
                if quote.messageId != nil {
                    Text(author).font(.system(.caption, weight: .semibold)).foregroundStyle(Theme.tx)
                }
                Text(excerpt).font(.system(.caption)).foregroundStyle(Theme.tx2).lineLimit(2)
            }
        }
        .fixedSize(horizontal: false, vertical: true)
        .padding(.horizontal, 10)
        .padding(.vertical, 6)
        .background(Theme.s1, in: RoundedRectangle(cornerRadius: 12))
        .overlay(RoundedRectangle(cornerRadius: 12).stroke(Theme.line))
        .accessibilityElement(children: .combine)
        .accessibilityLabel(String(localized: "In reply to \(author): \(excerpt)"))
    }

    private var author: String {
        if quote.outgoing { return String(localized: "You") }
        if let a = quote.author { return model.name(of: a) }
        return model.title(of: chat)
    }

    private var excerpt: String {
        guard quote.messageId != nil else { return String(localized: "A message not on this phone") }
        switch quote.kind {
        case .text: return quote.excerpt
        case .voice: return String(localized: "Voice message")
        case .photo: return String(localized: "Photo")
        case .other: return String(localized: "Message")
        }
    }
}

/// The reactions to a message, by emoji; tapping one adds or removes ours.
private struct ReactionChips: View {
    let message: StoredMessage
    @Binding var error: String?
    @Environment(AppModel.self) private var model

    var body: some View {
        HStack(spacing: 6) {
            ForEach(message.reactions, id: \.emoji) { r in
                Button {
                    error = model.react(message, r.mine ? "" : r.emoji)
                } label: {
                    Text(r.count > 1 ? "\(r.emoji) \(r.count)" : r.emoji)
                        .font(.system(.footnote))
                        .padding(.horizontal, 8)
                        .frame(minHeight: 28)
                        .background(Theme.s2, in: Capsule())
                        .overlay(Capsule().stroke(r.mine ? Theme.tx : Theme.line, lineWidth: r.mine ? 1.5 : 1))
                        .foregroundStyle(Theme.tx)
                }
                .buttonStyle(.plain)
                .accessibilityLabel(r.mine ? String(localized: "\(r.emoji), \(r.count), including yours") :
                    String(localized: "\(r.emoji), \(r.count)"))
            }
        }
    }
}

/// The reactions offered (§6.2).
let reactionChoices = ["👍", "❤️", "😂", "😮", "😢", "🙏"]

private struct MessageView: View {
    let message: StoredMessage
    let isGroup: Bool
    @Binding var error: String?
    var highlighted = false
    var reply: () -> Void = {}
    var openQuote: (Int64) -> Void = { _ in }
    @Environment(AppModel.self) private var model
    @State private var selecting = false

    var body: some View {
        VStack(alignment: message.outgoing ? .trailing : .leading, spacing: 3) {
            if let q = message.quote {
                Button { if let id = q.messageId { openQuote(id) } } label: {
                    QuoteBox(quote: q, chat: message.chat)
                }
                .buttonStyle(.plain)
                .disabled(q.messageId == nil)
            }
            bubble
                .contextMenu { actions }
            if !message.reactions.isEmpty {
                ReactionChips(message: message, error: $error)
            }
            if message.outgoing, !isGroup, case let .sending(sent, total) = message.status, total > 1 {
                FragmentProgress(sent: Int(sent), total: Int(total))
                    .frame(width: 220)
                    .padding(.top, 2)
            }
            // When it was sent (ours) or received, then how it went.
            HStack(spacing: 4) {
                if message.isWaiting {
                    Image(systemName: "clock").accessibilityHidden(true)
                }
                Text(footer)
            }
            .font(.system(.caption2))
            .foregroundStyle(Theme.tx2)
        }
        .frame(maxWidth: .infinity, alignment: message.outgoing ? .trailing : .leading)
        .padding(message.outgoing ? .leading : .trailing, 64)
        // A text reads as one: author, text, time and status. Media keep their buttons.
        .accessibilityElement(children: isText ? .combine : .contain)
        .padding(4)
        .background(RoundedRectangle(cornerRadius: 22).fill(highlighted ? Theme.warnBg : .clear))
        .padding(-4)
        .sheet(isPresented: $selecting) {
            if case let .text(_, text) = message.message { SelectTextSheet(text: text) }
        }
    }

    private var isText: Bool {
        if case .text = message.message { return true }
        return false
    }

    /// "≈ 2.1 s of airtime": a reaction costs about a short text (§6.2).
    private var reactionCost: String {
        guard let e = model.reactionCost(message.chat) else { return "" }
        return String(localized: "≈ \(String(format: "%.1f", Double(e.airtimeMs) / 1000)) s of airtime")
    }

    /// Long press: copy or select a text; cancel one that is still waiting, send a failed one
    /// again.
    @ViewBuilder private var actions: some View {
        if message.referable {
            Button(action: reply) { Label("Reply", systemImage: "arrowshape.turn.up.left") }
            Menu {
                Section(reactionCost) {
                    ForEach(reactionChoices, id: \.self) { e in
                        Button(e) { error = model.react(message, e) }
                    }
                    if message.reactions.contains(where: \.mine) {
                        Button("Remove my reaction", role: .destructive) { error = model.react(message, "") }
                    }
                }
            } label: {
                Label("React", systemImage: "face.smiling")
            }
        }
        if case let .text(_, text) = message.message {
            Button { UIPasteboard.general.string = text } label: { Label("Copy", systemImage: "doc.on.doc") }
            Button { selecting = true } label: { Label("Select text", systemImage: "selection.pin.in.out") }
        }
        if case let .waiting(_, started, _) = message.status, !started {
            Button(role: .destructive) { error = model.cancel(message) } label: {
                Label("Cancel sending", systemImage: "xmark.circle")
            }
        }
        if message.outgoing, case .failed = message.status {
            switch message.message {
            case .text, .voice:
                Button { error = model.retry(message) } label: {
                    Label("Send again", systemImage: "arrow.clockwise")
                }
            default:
                EmptyView()
            }
        }
    }

    private var footer: String {
        let time = message.date.formatted(date: .omitted, time: .shortened)
        if message.outgoing {
            return ([time] + [caption].compactMap { $0 }).joined(separator: " · ")
        }
        // Written well before it arrived: it waited (§6.1).
        if let written = message.writtenEarlier {
            if Calendar.current.isDate(written, inSameDayAs: message.date) {
                return String(localized: "\(time) · written at \(written.formatted(date: .omitted, time: .shortened))")
            }
            return String(localized: "\(time) · written on \(written.formatted(date: .abbreviated, time: .shortened))")
        }
        return time
    }

    @ViewBuilder private var bubble: some View {
        VStack(alignment: message.outgoing ? .trailing : .leading, spacing: 3) {
            if isGroup, !message.outgoing, let a = message.author {
                Text(model.name(of: a)).font(.system(.caption, weight: .semibold)).foregroundStyle(Theme.tx2)
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
                    .font(.system(.callout))
                    .padding(.horizontal, 14)
                    .padding(.vertical, 9)
                    .background(message.outgoing ? Theme.actBg : Theme.s2, in: RoundedRectangle(cornerRadius: 18))
                    .foregroundStyle(message.outgoing ? Theme.actFg : Theme.tx)
            }
        }
    }

    private var caption: String? {
        switch message.status {
        case .received: return nil
        case let .waiting(reason, started, expiresAtMs):
            switch reason {
            case .radio:
                if started { return String(localized: "Paused: radio disconnected") }
                return String(localized: "Waiting for the radio · expires in \(timeLeft(expiresAtMs))")
            case .earlier: return String(localized: "After earlier messages")
            case .path: return String(localized: "Looking for a path")
            case .handshake: return String(localized: "Opening a secure session")
            case let .budget(atMs):
                let at = Date(timeIntervalSince1970: TimeInterval(atMs) / 1000)
                    .formatted(date: .omitted, time: .shortened)
                return String(localized: "Waiting for airtime · around \(at)")
            }
        case let .sending(sent, total):
            if isGroup { return String(localized: "Sending to the group") }
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

    /// "24 h", "35 min": how long a waiting text still has before it expires (§6.1).
    private func timeLeft(_ expiresAtMs: Int64) -> String {
        let s = max(0, Double(expiresAtMs) / 1000 - Date().timeIntervalSince1970)
        return s >= 3600 ? "\(Int((s / 3600).rounded(.up))) h" : "\(max(1, Int((s / 60).rounded(.up)))) min"
    }
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
                        .font(.system(.subheadline, weight: .bold))
                        .frame(width: 36, height: 36)
                        .background(Theme.lora, in: Circle())
                        .foregroundStyle(Color(hex: 0x141413))
                }
                .frame(minWidth: 44, minHeight: 44)
                .disabled(!playable)
                .accessibilityLabel(playing ? "Stop" : "Play the voice message")
                Waveform(seed: data.prefix(32), color: message.outgoing ? Theme.actFg.opacity(0.55) : Theme.tx2)
                VStack(alignment: .leading, spacing: 0) {
                    Text(clock(TimeInterval(durationMs) / 1000)).font(.system(.footnote, weight: .semibold).monospacedDigit())
                    Text(codecName(mode))
                        .font(.system(.caption2))
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
    @State private var viewing = false

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
                            .font(.system(.caption2))
                            .padding(.horizontal, 8)
                            .padding(.vertical, 3)
                            .background(Theme.bg.opacity(0.85), in: RoundedRectangle(cornerRadius: 6))
                            .padding(8)
                    }
                }
                .frame(width: 250, height: 250 * CGFloat(max(height, 1)) / CGFloat(max(width, 1)))
                .clipped()
                // Full screen, zoomable.
                .contentShape(Rectangle())
                .onTapGesture { viewing = true }
                .accessibilityAddTraits(.isButton)
                .accessibilityHint("Opens the photo full screen")
                VStack(alignment: .leading, spacing: 8) {
                    HStack {
                        Text("Photo").font(.system(.footnote, weight: .semibold))
                        Spacer()
                        Text(ByteCountFormatter.string(fromByteCount: Int64(full ? data.count : Int(fullLen)), countStyle: .file))
                            .font(.system(.footnote))
                            .foregroundStyle(Theme.tx2)
                    }
                    HStack(spacing: 6) {
                        TransportBadge(kind: .lora, label: full ? String(localized: "LoRa · full thumbnail") : String(localized: "LoRa · preview"))
                        if !full && !message.outgoing { Text("full one on request").font(.system(.caption2)).foregroundStyle(Theme.tx2) }
                    }
                    if let announce {
                        BridgeSection(announce: announce, standalone: false)
                    } else if !message.outgoing && !full && fullLen > 0 {
                        if model.requestedFull.contains(message.id) {
                            Label("Full image requested…", systemImage: "hourglass")
                                .font(.system(.footnote))
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
            .fullScreenCover(isPresented: $viewing) { PhotoViewer(message: message) }
            #if DEBUG
            // -screen photo: the viewer on the first photo received.
            .onAppear { if model.debugScreen == "photo", !message.outgoing { viewing = true } }
            #endif
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
    @Binding var replyTo: StoredMessage?
    @Environment(AppModel.self) private var model
    @State private var picked: PhotosPickerItem?
    @State private var picking = false
    @State private var image: PreparedImage?
    @State private var original: Data?
    @State private var preparing = false
    @State private var locating = false

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            if let r = replyTo {
                HStack(spacing: 8) {
                    Image(systemName: "arrowshape.turn.up.left").foregroundStyle(Theme.tx2)
                    VStack(alignment: .leading, spacing: 1) {
                        Text(replyAuthor(r)).font(.system(.caption, weight: .semibold))
                        Text(r.message.display).font(.system(.caption)).foregroundStyle(Theme.tx2).lineLimit(1)
                    }
                    Spacer()
                    Button { replyTo = nil } label: {
                        Image(systemName: "xmark").frame(width: 44, height: 44)
                    }
                    .accessibilityLabel("Cancel the reply")
                }
                .padding(.leading, 12)
                .foregroundStyle(Theme.tx)
            }
            if let share = model.share(of: chat) {
                Button { locating = true } label: {
                    Label(sharingText(share), systemImage: "location.fill")
                        .font(.system(.caption, weight: .semibold))
                        .foregroundStyle(Theme.tx2)
                }
                .padding(.leading, 52)
            }
            if !model.isConnected {
                Label("Radio not connected: messages wait for it, up to 24 h", systemImage: "clock")
                    .font(.system(.caption2))
                    .foregroundStyle(Theme.tx2)
                    .padding(.leading, 52)
            } else if let line = costLine {
                Text(line).font(.system(.caption2)).foregroundStyle(Theme.tx2).padding(.leading, 52)
            }
            if model.recorder.recording, model.recorder.chat == chat {
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
                            .font(.system(.body, weight: .semibold))
                            .frame(width: 44, height: 44)
                            .background(Theme.s2, in: Circle())
                            .foregroundStyle(Theme.tx)
                    }
                    .disabled(preparing)
                    .accessibilityLabel("Attach")
                    TextField("Message", text: $draft, axis: .vertical)
                        .lineLimit(1...5)
                        .font(.system(.callout))
                        .padding(.horizontal, 14)
                        .padding(.vertical, 9)
                        .background(Theme.bg, in: RoundedRectangle(cornerRadius: 22))
                        .overlay(RoundedRectangle(cornerRadius: 22).stroke(Theme.line))
                    if isContact && draft.isEmpty {
                        Button {
                            Task { await model.recorder.start(for: chat) }
                        } label: {
                            Image(systemName: "mic")
                                .font(.system(.title3, weight: .semibold))
                                .frame(width: 44, height: 44)
                                // Voice messages need the radio (§6.1): grey while it is away.
                                .foregroundStyle(model.isConnected ? Theme.lora : Theme.tx2)
                        }
                        .disabled(!model.isConnected)
                        .accessibilityLabel("Record a voice message")
                    } else {
                        Button {
                            let text = draft.trimmingCharacters(in: .whitespacesAndNewlines)
                            if let r = replyTo {
                                if let e = model.reply(text, to: r) { error = e } else { draft = ""; replyTo = nil }
                            } else if let e = model.send(text, to: chat) { error = e } else { draft = "" }
                        } label: {
                            Image(systemName: "arrow.up")
                                .font(.system(.body, weight: .bold))
                                .frame(width: 44, height: 44)
                                .background(Theme.actBg, in: Circle())
                                .foregroundStyle(Theme.actFg)
                        }
                        // Texts wait for the radio (§6.1); not with a throwaway identity.
                        .disabled(draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || model.identityProblem != nil)
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

    /// "Replying to Camille", "Replying to yourself".
    private func replyAuthor(_ m: StoredMessage) -> String {
        if m.outgoing { return String(localized: "Replying to yourself") }
        if let a = m.author { return String(localized: "Replying to \(model.name(of: a))") }
        return String(localized: "Replying to \(model.title(of: chat))")
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
                    .font(.system(.callout, weight: .bold))
                    .frame(width: 44, height: 44)
                    .background(Theme.s2, in: Circle())
                    .foregroundStyle(Theme.tx)
            }
            .accessibilityLabel("Cancel")
            Circle().fill(Theme.sos).frame(width: 10, height: 10)
            VStack(alignment: .leading, spacing: 1) {
                Text("\(clock(seconds)) / \(clock(VoiceRecorder.maxSeconds))")
                    .font(.system(.subheadline, weight: .semibold).monospacedDigit())
                if let e = estimate {
                    Text("\(fragmentsText(e.fragments)) · ≈ \(String(format: "%.0f", Double(e.airtimeMs) / 1000)) s of airtime")
                        .font(.system(.caption2))
                        .foregroundStyle(Theme.tx2)
                }
            }
            Spacer()
            Button {
                // To the conversation the recording was made in.
                guard let to = model.recorder.chat, let pcm = model.recorder.stop(keep: true) else { return }
                let voice = prepareVoice(pcm: pcm, mode: UInt8(model.voiceMode))
                if let e = model.sendVoice(voice, to: to) { error = e }
            } label: {
                Image(systemName: "arrow.up")
                    .font(.system(.body, weight: .bold))
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

    /// Before every radio send: what it costs us. A group message is flooded: relays add their
    /// own airtime, and nobody acknowledges it.
    private var costLine: String? {
        guard let e = model.estimate(chat, text: draft) else {
            if case .group = chat { return String(localized: "Flooded to every member, without acknowledgement") }
            return nil
        }
        let s = String(format: "%.1f", Double(e.airtimeMs) / 1000)
        if case .group = chat {
            return String(localized: "Your cost: \(fragmentsText(e.fragments)) · ≈ \(s) s of airtime · relays add theirs, no acknowledgement")
        }
        return String(localized: "Estimated cost: \(fragmentsText(e.fragments)) · ≈ \(s) s of airtime")
    }
}

/// Our position for this chat, from the composer's menu: the same shares as the map's panel.
struct ChatShareSheet: View {
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
                        .font(.system(.caption)).foregroundStyle(Theme.tx2)
                    SectionLabel("Duration").padding(.top, 8)
                    Picker("Duration", selection: $hours) {
                        Text("1 h").tag(1.0)
                        Text("8 h").tag(8.0)
                        Text("24 h").tag(24.0)
                    }
                    .pickerStyle(.segmented)
                    Text("Sharing stops on its own at the end; the others see your marker disappear.")
                        .font(.system(.caption)).foregroundStyle(Theme.tx2)
                    if case .group = chat {
                        Text("A group gets at most one position every 5 minutes.")
                            .font(.system(.caption)).foregroundStyle(Theme.tx2)
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
                                .font(.system(.caption))
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
                        .font(.system(.footnote))
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
                Text(title).font(.system(.subheadline, weight: .semibold))
                Text(detail).font(.system(.footnote)).foregroundStyle(Theme.tx2)
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
    @State private var viewing = false

    var body: some View {
        if case let .announce(_, _, _, hash, size, providers, name) = announce.message {
            let file = model.downloads[hash]
            let content = VStack(alignment: .leading, spacing: 8) {
                if standalone, let file, let ui = UIImage(contentsOfFile: file.path) {
                    Image(uiImage: ui).resizable().scaledToFit()
                        .clipShape(RoundedRectangle(cornerRadius: 12))
                        .onTapGesture { viewing = true }
                        .accessibilityAddTraits(.isButton)
                        .fullScreenCover(isPresented: $viewing) { PhotoViewer(message: announce) }
                }
                HStack {
                    Text(verbatim: name).font(.system(.footnote, weight: .semibold)).lineLimit(1)
                    Spacer()
                    Text(ByteCountFormatter.string(fromByteCount: Int64(size), countStyle: .file))
                        .font(.system(.footnote)).foregroundStyle(Theme.tx2)
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
                            .font(.system(.caption2))
                            .foregroundStyle(Theme.ok)
                    }
                }
                if let error = model.fetchErrors[hash] {
                    Text(error).font(.system(.caption)).foregroundStyle(Theme.sosFg)
                }
                if file == nil && !announce.outgoing {
                    if let via = model.fetching[hash] {
                        HStack(spacing: 8) {
                            ProgressView()
                            Text(via == 2 ? String(localized: "Downloading over Tor: it can take a minute…")
                                          : String(localized: "Downloading over iroh…"))
                                .font(.system(.footnote)).foregroundStyle(Theme.tx2)
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

/// A message's text, to select part of it; links, addresses and phone numbers can be tapped.
private struct SelectTextSheet: View {
    let text: String
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            SelectableText(text: text)
                .padding(.horizontal, 12)
                .screen()
                .navigationTitle("Select text")
                .navigationBarTitleDisplayMode(.inline)
                .toolbar {
                    ToolbarItem(placement: .confirmationAction) { Button("Done") { dismiss() } }
                }
        }
        .presentationDetents([.medium, .large])
    }
}

private struct SelectableText: UIViewRepresentable {
    let text: String

    func makeUIView(context: Context) -> UITextView {
        let v = UITextView()
        v.isEditable = false
        v.isSelectable = true
        v.dataDetectorTypes = [.link, .phoneNumber, .address]
        v.backgroundColor = .clear
        v.font = .preferredFont(forTextStyle: .body)
        v.adjustsFontForContentSizeCategory = true
        v.textColor = .label
        return v
    }

    func updateUIView(_ v: UITextView, context: Context) {
        v.text = text
    }
}
