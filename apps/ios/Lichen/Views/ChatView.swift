import PhotosUI
import QuickLook
import SwiftUI
import UniformTypeIdentifiers

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
    /// What a MeshCore chat is (§15.6).
    @State private var plainInfo = false
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
    /// Our message being edited (§6.4).
    @State private var editing: StoredMessage?
    /// A quoted original to go to.
    @State private var jumpTo: Int64?
    /// The view stays on the last message while the content grows (photos, stickers and GIFs
    /// laid out after the first pass): from opening at the end until the user scrolls away.
    @State private var pinned = false
    /// Blocking the other side, to confirm (ADR-0034).
    @State private var blocking = false

    var body: some View {
        VStack(spacing: 0) {
            // A new key waiting for the user stays in sight, above the messages.
            if case let .contact(key) = chat, let pending = model.conflicts[key] {
                Banner(kind: .sos, title: String(localized: "Another identity answers for \(model.title(of: chat))"),
                       message: String(localized: "Another identity now answers for their radio: a reinstalled app, or someone else. Your messages still go to the one you know. Compare safety numbers before you accept the new one."),
                       action: (String(localized: "Compare"), { verifying = pending }))
                    .padding(.horizontal, 14)
                    .padding(.top, 8)
            }
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
                            if case .notice = m.message {
                                NoticeLine(message: m.message).id(m.id)
                            } else if case let .announce(_, mediaId, _, _, _, _, _) = m.message,
                                      model.preview(for: mediaId, in: chat) != nil {
                                // Shown inside its LoRa preview's card: only its place here.
                                Color.clear.frame(height: 0).id(m.id)
                            } else {
                                MessageView(message: m, isGroup: isGroup || isChannel || isRoom, error: $error, highlighted: highlight == m.id,
                                            reply: { replyTo = m; editing = nil },
                                            edit: {
                                                editing = m
                                                replyTo = nil
                                                if case let .text(_, text) = m.message { draft = text }
                                            },
                                            openQuote: { jumpTo = $0 })
                                    .id(m.id)
                            }
                        }
                        if let failed = lastFailure {
                            FailureBanner(message: failed) { error = model.retry(failed) }
                                .padding(.top, 4)
                        }
                        Color.clear.frame(height: 1)
                            .accessibilityElement()
                            .accessibilityLabel("End of conversation")
                            .accessibilityIdentifier("chat.end")
                            .id(Self.bottom)
                    }
                    .scrollTargetLayout()
                    .padding(.horizontal, 14)
                    .padding(.vertical, 12)
                }
                .scrollPosition(id: $bottomVisible, anchor: .bottom)
                .defaultScrollAnchor(.bottom)
                .modifier(FollowBottom(pinned: $pinned) { proxy.scrollTo(Self.bottom, anchor: .bottom) })
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
                    pinned = highlight == nil && dividerBefore == nil
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
                    guard model.loadUntil(chat, id) else {
                        error = String(localized: "This message is no longer on this phone.")
                        return
                    }
                    highlight = id
                    Task {
                        try? await Task.sleep(for: .milliseconds(100))
                        withAnimation { proxy.scrollTo(id, anchor: .center) }
                        try? await Task.sleep(for: .seconds(2))
                        withAnimation { if highlight == id { highlight = nil } }
                    }
                }
            }
            if let node = chatKey, model.isBlocked(node) {
                // Nothing to write with: they are blocked (ADR-0034).
                BlockedBar(name: model.title(of: chat)) { model.unblock(node) }
            } else {
                Composer(chat: chat, draft: $draft, error: $error, replyTo: $replyTo, editing: $editing)
            }
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
                .onTapGesture { if chatKey != nil { renaming = true } }
                .accessibilityAddTraits(chatKey == nil ? [] : .isButton)
            }
            if isGroup {
                ToolbarItem(placement: .topBarTrailing) {
                    Button { showingGroup = true } label: { Image(systemName: "info.circle") }
                        .accessibilityLabel("Group details")
                }
            } else if case let .contact(key) = chat, let c = model.contact(key), c.identity != nil {
                // Their key, always within reach: to verify, verified, or changed.
                ToolbarItem(placement: .topBarTrailing) {
                    let pending = model.conflicts[key]
                    Button { verifying = pending ?? Data() } label: {
                        Image(systemName: pending != nil ? "exclamationmark.shield" : c.verified ? "lock" : "lock.open")
                            .foregroundStyle(pending != nil ? Theme.sos : c.verified ? Theme.tx2 : Theme.warn)
                    }
                    .accessibilityLabel(pending != nil ? String(localized: "Their key changed: compare")
                                        : c.verified ? String(localized: "Key verified")
                                        : String(localized: "Verify their key"))
                }
            } else if chat.isPlain {
                ToolbarItem(placement: .topBarTrailing) {
                    Button { plainInfo = true } label: { Image(systemName: "info.circle") }
                        .accessibilityLabel(isChannel ? String(localized: "Channel details")
                                            : isRoom ? String(localized: "Room details")
                                            : String(localized: "About MeshCore chats"))
                }
            }
            ToolbarItem(placement: .topBarTrailing) { timerMenu }
            ToolbarItem(placement: .topBarTrailing) { moreMenu }
        }
        .confirmationDialog(String(localized: "Block \(model.title(of: chat))?"), isPresented: $blocking,
                            titleVisibility: .visible) {
            Button("Block", role: .destructive) {
                if let node = chatKey { error = model.block(node) }
            }
        } message: {
            Text("Their messages, introductions and group posts no longer reach this phone, and their posts already in your groups are hidden. They are not told. Their position and SOS in a group still show.")
        }
        .sheet(isPresented: $plainInfo) { PlainInfoSheet(chat: chat) }
        .navigationDestination(isPresented: $showingGroup) {
            if case let .group(gid) = chat { GroupView(gid: gid) }
        }
        .onAppear {
            model.load(chat)
            if let f = model.focus, f.chat == chat {
                model.focus = nil
                if model.loadUntil(chat, f.id) {
                    highlight = f.id
                } else {
                    error = String(localized: "This message is no longer on this phone.")
                }
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
            if model.debugScreen == "group" || model.debugScreen == "invite", isGroup { showingGroup = true }
            // -timer N: this conversation's messages disappear after N seconds (§6.5).
            let timer = UserDefaults.standard.integer(forKey: "timer")
            if timer > 0, model.timers[chat] != UInt32(timer) { _ = model.setTimer(chat, UInt32(timer)) }
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
        switch chat {
        case let .contact(key), let .mesh(key): key
        case .group, .channel, .room: nil
        }
    }

    private var isChannel: Bool {
        if case .channel = chat { return true }
        return false
    }

    /// A room server's posts (protocol.md §15.8).
    private var isRoom: Bool {
        if case .room = chat { return true }
        return false
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
            if model.conflicts[key] != nil {
                // Pinned above the list instead.
                EmptyView()
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
        } else if chat.isPlain {
            // Not lichen-encrypted, said where the lock would be (§15.6).
            Button { plainInfo = true } label: {
                Label(isChannel ? String(localized: "MeshCore channel · not lichen-encrypted")
                                : isRoom ? String(localized: "MeshCore room · not lichen-encrypted")
                                : String(localized: "MeshCore · not lichen-encrypted"),
                      systemImage: "lock.slash")
                    .font(.system(.caption))
                    .foregroundStyle(Theme.warnFg)
                    .labelStyle(TintedIconLabel(tint: Theme.warn))
            }
            .buttonStyle(.plain)
            .padding(.top, 4)
        } else if isGroup {
            Label("Signed group messages, flooded", systemImage: "lock")
                .font(.system(.caption))
                .foregroundStyle(Theme.tx2)
                .labelStyle(TintedIconLabel(tint: Theme.ok))
                .padding(.top, 4)
        }
    }

    /// Archive and block (ADR-0034).
    private var moreMenu: some View {
        let archived = model.isArchived(chat)
        return Menu {
            Button {
                model.setArchived(chat, !archived)
                if !archived { dismissChat() }
            } label: {
                Label(archived ? "Unarchive" : "Archive", systemImage: archived ? "tray.and.arrow.up" : "archivebox")
            }
            if let node = chatKey {
                if model.isBlocked(node) {
                    Button { model.unblock(node) } label: { Label("Unblock", systemImage: "hand.raised.slash") }
                } else {
                    Button(role: .destructive) { blocking = true } label: { Label("Block", systemImage: "hand.raised") }
                }
            }
        } label: {
            Image(systemName: "ellipsis.circle").foregroundStyle(Theme.tx2)
        }
        .accessibilityLabel("More")
    }

    /// Disappearing messages (§6.5): the choices, and what the timer cannot do.
    private var timerMenu: some View {
        let current = model.timers[chat] ?? 0
        return Menu {
            Picker("Disappearing messages", selection: Binding(get: { current }, set: { error = model.setTimer(chat, $0) })) {
                ForEach(AppModel.timerChoices, id: \.self) { s in
                    Text(s == 0 ? String(localized: "Off") : timerLabel(s)).tag(s)
                }
            }
            Section {
                Text(chat.isPlain
                     ? String(localized: "On this phone only: MeshCore apps keep their copy. Screenshots can keep messages too.")
                     : String(localized: "New messages disappear on every phone that gets the setting. Screenshots and modified apps can still keep them."))
            }
        } label: {
            Image(systemName: current == 0 ? "timer" : "timer.circle.fill")
                .foregroundStyle(current == 0 ? Theme.tx2 : Theme.tx)
        }
        .accessibilityLabel(current == 0 ? String(localized: "Disappearing messages: off")
                            : String(localized: "Disappearing messages: \(timerLabel(current))"))
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
        case let .mesh(key):
            let hops = hopsBadge(model.contacts.first { $0.key == key }?.hops)
            return String(localized: "MeshCore · \(hops)")
        case let .channel(id):
            return model.channels.first { $0.id == id }?.kind.label ?? String(localized: "MeshCore channel")
        case let .room(key):
            let login = model.server(key)?.login ?? .out
            return String(localized: "MeshCore room server · \(login.label)")
        }
    }
}

/// A membership change in a group (ADR-0021): "Léa joined", "Camille removed Bob".
struct NoticeLine: View {
    let message: Message
    @Environment(AppModel.self) private var model

    var body: some View {
        Text(message.noticeText(me: model.nodeKey) { node, name in model.contact(node)?.name ?? name })
            .font(.system(.caption))
            .foregroundStyle(Theme.tx2)
            .multilineTextAlignment(.center)
            .frame(maxWidth: .infinity)
            .padding(.horizontal, 24)
            .padding(.vertical, 4)
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
        case .file: return String(localized: "File: \(quote.excerpt)")
        case .sticker: return String(localized: "Sticker \(quote.excerpt)")
        case .gif: return quote.excerpt.isEmpty ? String(localized: "GIF") : String(localized: "GIF: \(quote.excerpt)")
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
    var edit: () -> Void = {}
    var openQuote: (Int64) -> Void = { _ in }
    @Environment(AppModel.self) private var model
    @State private var selecting = false
    /// Deleting for everyone (true) or for this phone (false), to confirm.
    @State private var deleting: Bool?
    @State private var forwarding = false

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
                .sheet(isPresented: $forwarding) { ForwardSheet(message: message) }
                .confirmationDialog(deleting == true ? String(localized: "Delete for everyone?") : String(localized: "Delete for you?"),
                                    isPresented: Binding(get: { deleting != nil }, set: { if !$0 { deleting = nil } }),
                                    titleVisibility: .visible) {
                    if deleting == true {
                        Button("Delete for everyone", role: .destructive) { error = model.deleteForEveryone(message) }
                    } else {
                        Button("Delete for me", role: .destructive) { error = model.deleteForMe(message) }
                    }
                } message: {
                    Text(deleting == true
                         ? String(localized: "It is removed on the phones that receive this. Whoever already read it has read it.")
                         : String(localized: "Only on this phone. The others keep it."))
                }
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
            // Coordinates in the text (ADR-0034).
            if let place = LinkedText.places(in: text).first {
                Button { model.showOnMap(lat: place.lat, lon: place.lon) } label: {
                    Label("Show on the map", systemImage: "map")
                }
            }
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
        // A new message of ours with the same content (ADR-0027).
        switch message.message {
        case .text, .sticker, .gif, .file, .voice:
            Button { forwarding = true } label: { Label("Forward", systemImage: "arrowshape.turn.up.right") }
        default:
            EmptyView()
        }
        // Ours within 24 hours (§6.4).
        if message.editable {
            Button(action: edit) { Label("Edit", systemImage: "pencil") }
        }
        if message.deletable {
            Button(role: .destructive) { deleting = true } label: {
                Label("Delete for everyone", systemImage: "trash")
            }
        }
        Button(role: .destructive) { deleting = false } label: {
            Label("Delete for me", systemImage: "trash.slash")
        }
    }

    private var footer: String {
        let time = message.date.formatted(date: .omitted, time: .shortened)
        let edited = message.edited ? String(localized: "edited") : nil
        if message.outgoing {
            return ([time, edited] + [caption]).compactMap { $0 }.joined(separator: " · ")
        }
        if let edited { return "\(time) · \(edited)" }
        // Lost on the air and handed on by a member (§10.6): shown where it arrived.
        if message.late {
            let written = (message.writtenEarlier ?? message.message.date).formatted(date: .omitted, time: .shortened)
            return String(localized: "\(time) · arrived late, written at \(written)")
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
            // A group member, or a channel post's author as its radio calls itself (§15.1).
            if isGroup, !message.outgoing,
               let name = message.authorName ?? message.author.map({ model.name(of: $0) }) {
                Text(name).font(.system(.caption, weight: .semibold)).foregroundStyle(Theme.tx2)
                    .padding(.horizontal, 6)
            }
            switch message.message {
            case .voice:
                VoiceNote(message: message)
            case .image:
                ImageCard(message: message)
            case .file:
                FileCard(message: message)
            case let .sticker(_, _, emoji):
                StickerView(emoji: emoji)
            case .gif:
                GifCard(message: message)
            case let .announce(_, mediaId, _, _, _, _, _):
                // Shown inside its LoRa preview's card when that preview is here.
                if model.preview(for: mediaId, in: message.chat) == nil {
                    BridgeSection(announce: message, standalone: true)
                }
            case .deleted:
                // Taken back by its author (§6.4): its place stays.
                Label("Message deleted", systemImage: "trash")
                    .font(.system(.callout).italic())
                    .padding(.horizontal, 14)
                    .padding(.vertical, 9)
                    .overlay(RoundedRectangle(cornerRadius: 18).stroke(Theme.line))
                    .foregroundStyle(Theme.tx2)
            case let .text(_, text):
                // Links open in the browser when tapped, places on the map (ADR-0034).
                LinkedText(text: text, outgoing: message.outgoing)
                    .font(.system(.callout))
                    .padding(.horizontal, 14)
                    .padding(.vertical, 9)
                    .background(message.outgoing ? Theme.actBg : Theme.s2, in: RoundedRectangle(cornerRadius: 18))
                    .foregroundStyle(message.outgoing ? Theme.actFg : Theme.tx)
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
            if case .channel = message.chat { return String(localized: "Sending to the channel") }
            if case .mesh = message.chat { return String(localized: "Waiting for their radio's acknowledgement") }
            if case .room = message.chat { return String(localized: "Waiting for the room's acknowledgement") }
            if isGroup { return String(localized: "Sending to the group") }
            return total > 1 ? String(localized: "Waiting for the acknowledgement") :
                String(localized: "Sending \(min(Int(sent), Int(total))) / \(Int(total)) fragments")
        case .delivered:
            if case .mesh = message.chat { return String(localized: "Delivered · \(airtime) s of airtime") }
            if case .room = message.chat { return String(localized: "Kept by the room · \(airtime) s of airtime") }
            return String(localized: "Delivered · \(Int(message.fragments)) fragments · \(airtime) s of airtime")
        case let .failed(reason):
            return String(localized: "Not delivered: \(reason.display)")
        case .flooded:
            if case .channel = message.chat { return String(localized: "Sent to the channel") }
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
            let total = TimeInterval(durationMs) / 1000
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
                // The waveform fills as it plays; a tap or a drag on it goes there.
                let base = message.outgoing ? Theme.actFg.opacity(0.45) : Theme.tx2.opacity(0.55)
                let done = message.outgoing ? Theme.actFg : Theme.tx
                Waveform(seed: data.prefix(32), color: base)
                    .overlay(alignment: .leading) {
                        GeometryReader { g in
                            Waveform(seed: data.prefix(32), color: done)
                                .mask(alignment: .leading) {
                                    Rectangle().frame(width: g.size.width * (playing ? model.player.progress : 0))
                                }
                        }
                    }
                    .contentShape(Rectangle())
                    .gesture(DragGesture(minimumDistance: 0).onChanged { v in
                        guard playable else { return }
                        let f = v.location.x / 110
                        if playing { model.player.seek(f) } else { model.player.play(message, from: f) }
                    })
                    .frame(width: 110)
                    .accessibilityElement()
                    .accessibilityLabel("Position")
                    .accessibilityValue(clock(playing ? model.player.position : 0))
                    .accessibilityAdjustableAction { dir in
                        guard playing else { return }
                        let step = 2 / max(total, 1)
                        model.player.seek(model.player.progress + (dir == .increment ? step : -step))
                    }
                VStack(alignment: .leading, spacing: 0) {
                    Text(playing ? "\(clock(model.player.position)) / \(clock(total))" : clock(total))
                        .font(.system(.footnote, weight: .semibold).monospacedDigit())
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
                return DecodedImages.shared.image(url: url, maxPixel: 2048)
            }
            VStack(alignment: .leading, spacing: 0) {
                ZStack(alignment: .bottomLeading) {
                    if let fetched {
                        Image(uiImage: fetched)
                            .resizable()
                            .aspectRatio(CGFloat(max(width, 1)) / CGFloat(max(height, 1)), contentMode: .fill)
                    } else if let ui = DecodedImages.shared.image(data: data, maxPixel: 480) {
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
    @Binding var editing: StoredMessage?
    @Environment(AppModel.self) private var model
    @State private var picked: PhotosPickerItem?
    @State private var picking = false
    @State private var image: PreparedImage?
    @State private var original: Data?
    @State private var preparing = false
    @State private var locating = false
    /// A file picked, waiting for its route (LoRa when small, else the bridge, ADR-0022).
    @State private var importing = false
    @State private var file: FileDraft?
    /// A recording stopped and not sent yet: listened to, its quality chosen, sent or dropped.
    @State private var review: Review?
    /// The sticker picker (§6.3).
    @State private var stickers = false
    /// A GIPHY link typed or pasted is sent as a GIF (§6.3) unless this is set; its first
    /// frame's size, once loaded, goes with it.
    @State private var linkAsText = false
    @State private var gifData: Data?

    struct Review {
        let pcm: [Int16]
        var mode: UInt8
        var voice: PreparedVoice
    }

    /// The draft is a GIPHY link (§6.3).
    private var gifLink: GifLink? { chat.isPlain || editing != nil ? nil : Lichen.gifLink(text: draft) }

    /// Bytes over a plain chat's limit (§15.2, §15.3); 0 within it.
    private var plainOver: Int {
        chat.isPlain ? max(0, draft.utf8.count - model.plainTextMax(chat)) : 0
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            if editing != nil {
                HStack(spacing: 8) {
                    Image(systemName: "pencil").foregroundStyle(Theme.tx2)
                    Text("Editing your message").font(.system(.caption, weight: .semibold))
                    Spacer()
                    Button { editing = nil; draft = "" } label: {
                        Image(systemName: "xmark").frame(width: 44, height: 44)
                    }
                    .accessibilityLabel("Cancel the edit")
                }
                .padding(.leading, 12)
                .foregroundStyle(Theme.tx)
            }
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
            if let link = gifLink, !linkAsText {
                HStack(spacing: 10) {
                    ZStack {
                        Theme.s2
                        if let gifData { AnimatedImage(data: gifData) } else { ProgressView() }
                    }
                    .frame(width: 64, height: 48)
                    .clipShape(RoundedRectangle(cornerRadius: 8))
                    VStack(alignment: .leading, spacing: 1) {
                        Text(link.title.isEmpty ? String(localized: "GIF") : String(localized: "GIF: \(link.title)"))
                            .font(.system(.caption, weight: .semibold)).lineLimit(1)
                        Text("Sent as a GIF: a few bytes instead of the link")
                            .font(.system(.caption)).foregroundStyle(Theme.tx2).lineLimit(2)
                    }
                    Spacer()
                    Button { linkAsText = true } label: {
                        Image(systemName: "xmark").frame(width: 44, height: 44)
                    }
                    .accessibilityLabel("Send the link as text")
                }
                .padding(.leading, 12)
                .foregroundStyle(Theme.tx)
                .task(id: link.id) {
                    gifData = nil
                    gifData = await model.loadGif(link.id)
                }
            }
            if let share = model.share(of: chat) {
                Button { locating = true } label: {
                    Label(sharingText(share), systemImage: "location.fill")
                        .font(.system(.caption, weight: .semibold))
                        .foregroundStyle(Theme.tx2)
                }
                .padding(.leading, 52)
            }
            if case .channel = chat, !model.isConnected {
                Label("Radio not connected: channel posts need it", systemImage: "antenna.radiowaves.left.and.right.slash")
                    .font(.system(.caption2))
                    .foregroundStyle(Theme.tx2)
                    .padding(.leading, 52)
            } else if !model.isConnected {
                Label("Radio not connected: messages wait for it, up to 24 h", systemImage: "clock")
                    .font(.system(.caption2))
                    .foregroundStyle(Theme.tx2)
                    .padding(.leading, 52)
            } else if let line = costLine {
                Text(line).font(.system(.caption2)).foregroundStyle(Theme.tx2).padding(.leading, 52)
            }
            if model.recorder.recording, model.recorder.chat == chat {
                recordingBar
            } else if let r = review {
                reviewBar(r)
            } else {
                HStack(spacing: 8) {
                    // A MeshCore chat carries text only (§15).
                    if !chat.isPlain {
                    Menu {
                        if isContact || !model.bridgeProviders(anonymous: false).isEmpty {
                            Button { picking = true } label: { Label("Photo", systemImage: "photo") }
                                .disabled(!model.isConnected)
                        }
                        if isContact || !model.bridgeProviders(anonymous: false).isEmpty {
                            Button { importing = true } label: { Label("File", systemImage: "doc") }
                        }
                        Button { stickers = true } label: { Label("Sticker", systemImage: "face.smiling") }
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
                    }
                    TextField("Message", text: $draft, axis: .vertical)
                        .accessibilityIdentifier("composer")
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
                            if let link = gifLink, !linkAsText {
                                let size = gifData.flatMap(AnimatedFrames.size)
                                if let e = model.sendGif(link, size: size, to: chat) { error = e } else { draft = ""; gifData = nil }
                            } else if let m = editing {
                                if let e = model.edit(text, of: m) { error = e } else { draft = ""; editing = nil }
                            } else if let r = replyTo {
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
                        .disabled(draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || model.identityProblem != nil
                                  || plainOver > 0 || (isChannelChat && !model.isConnected))
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
        .sheet(isPresented: $stickers) {
            StickerSheet { emoji in error = model.sendSticker(emoji, to: chat) }
        }
        .onChange(of: draft) { if gifLink == nil { linkAsText = false } }
        .fileImporter(isPresented: $importing, allowedContentTypes: [.item]) { result in
            guard case let .success(url) = result else { return }
            preparing = true
            Task {
                defer { preparing = false }
                // Off the main thread, and never more than the bridge carries (recheck R04).
                switch await Task.detached(operation: { readPicked(url) }).value {
                case let .success(draft): file = draft
                case let .failure(problem): error = problem.message
                }
            }
        }
        .sheet(item: $file) { draft in
            SendFileSheet(chat: chat, draft: draft) { e in
                file = nil
                error = e
            }
        }
        .onChange(of: picked) {
            guard let item = picked else { return }
            picked = nil
            preparing = true
            Task {
                defer { preparing = false }
                // Decoded and scaled in the background (SR-27): a picked photo can be large.
                guard let data = try? await item.loadTransferable(type: Data.self),
                      let (rgba, w, h) = await Task.detached(priority: .userInitiated, operation: {
                          BoundedImage.from(data: data, maxPixel: 2048)?.rgba()
                      }).value
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
            // -screen file / fileBig: the file sheet with a 3 KB note, or a 240 KB document.
            if model.debugScreen == "file" || model.debugScreen == "fileBig", file == nil {
                let big = model.debugScreen == "fileBig"
                let line = Data("Refuge de la Pra: code 4521, wood in the shed, water 200 m north.\n".utf8)
                file = FileDraft(name: big ? "Topo Vercors 2026.pdf" : "Refuge notes.txt",
                                 data: Data(repeating: 0, count: 0) + Array(repeating: line, count: big ? 3_700 : 45).reduce(Data(), +))
            }
            // -screen voiceReview: the review of a 4 s recording (a hum).
            if model.debugScreen == "voiceReview", review == nil {
                let pcm = (0..<32_000).map { i -> Int16 in
                    let t = Double(i) / 8_000
                    return Int16(6_000 * sin(2 * .pi * (150 + 30 * sin(t * 3)) * t) * (0.6 + 0.4 * sin(t * 9)))
                }
                review = Review(pcm: pcm, mode: 2, voice: prepareVoice(pcm: pcm, mode: 2))
            }
            // -screen media: the photo sheet with the demo landscape.
            if model.debugScreen == "media", isContact, image == nil,
               let ui = UIImage(named: "AppIcon") ?? Self.sampleImage(), let (rgba, w, h) = ui.rgba() {
                image = try? prepareImage(rgba: rgba, width: w, height: h)
            }
            #endif
        }
        .sheet(isPresented: $locating) { ChatShareSheet(chat: chat) }
        // A recording under review belongs to this conversation: leaving it stops the playback.
        .onDisappear { if model.player.playing == VoicePlayer.review { model.player.stop() } }
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
                // Stop and listen before sending.
                guard let pcm = model.recorder.stop(keep: true) else { return }
                let mode = UInt8(model.voiceMode)
                review = Review(pcm: pcm, mode: mode, voice: prepareVoice(pcm: pcm, mode: mode))
            } label: {
                Image(systemName: "stop.fill")
                    .font(.system(.body, weight: .bold))
                    .frame(width: 44, height: 44)
                    .background(Theme.s2, in: Circle())
                    .foregroundStyle(Theme.tx)
            }
            .accessibilityLabel("Stop and listen")
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

    /// A stopped recording: play what will be sent (encoded, then decoded: Codec2's real
    /// sound), choose its quality against its airtime, drop it or send it.
    private func reviewBar(_ r: Review) -> some View {
        let reviewing = model.player.playing == VoicePlayer.review
        let modes: [UInt8] = [0, 2, 3]
        let cost = model.estimate(chat, datagramLen: voiceDatagramLenFor(mode: r.mode, durationMs: r.voice.durationMs))
        return VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 10) {
                Button(role: .destructive) {
                    model.player.stop()
                    review = nil
                } label: {
                    Image(systemName: "trash")
                        .font(.system(.callout, weight: .semibold))
                        .frame(width: 44, height: 44)
                        .background(Theme.s2, in: Circle())
                        .foregroundStyle(Theme.sos)
                }
                .accessibilityLabel("Discard the recording")
                Button {
                    if reviewing {
                        model.player.stop()
                    } else if let pcm = decodeVoice(mode: r.voice.mode, frames: r.voice.frames, data: r.voice.data) {
                        model.player.play(pcm: pcm, id: VoicePlayer.review)
                    }
                } label: {
                    Image(systemName: reviewing ? "stop.fill" : "play.fill")
                        .font(.system(.subheadline, weight: .bold))
                        .frame(width: 44, height: 44)
                        .background(Theme.lora, in: Circle())
                        .foregroundStyle(Color(hex: 0x141413))
                }
                .accessibilityLabel(reviewing ? "Stop" : "Listen to what will be sent")
                Text(reviewing ? "\(clock(model.player.position)) / \(clock(TimeInterval(r.voice.durationMs) / 1000))"
                               : clock(TimeInterval(r.voice.durationMs) / 1000))
                    .font(.system(.subheadline, weight: .semibold).monospacedDigit())
                Spacer()
                Button {
                    model.player.stop()
                    if let e = model.sendVoice(r.voice, to: chat) { error = e } else { review = nil }
                } label: {
                    Image(systemName: "arrow.up")
                        .font(.system(.body, weight: .bold))
                        .frame(width: 44, height: 44)
                        .background(Theme.actBg, in: Circle())
                        .foregroundStyle(Theme.actFg)
                }
                .accessibilityLabel("Send")
            }
            // Voice quality against time on air, for this message.
            Picker("Voice quality", selection: Binding(get: { r.mode }, set: { m in
                model.player.stop()
                review = Review(pcm: r.pcm, mode: m, voice: prepareVoice(pcm: r.pcm, mode: m))
            })) {
                ForEach(modes, id: \.self) { m in Text(codecName(m)).tag(m) }
            }
            .pickerStyle(.segmented)
            if let e = cost {
                Text("Voice quality vs time on air: \(fragmentsText(e.fragments)) · ≈ \(String(format: "%.0f", Double(e.airtimeMs) / 1000)) s")
                    .font(.system(.caption2))
                    .foregroundStyle(Theme.tx2)
            }
        }
    }

    private func sharingText(_ share: ShareInfo) -> String {
        let until = Date(timeIntervalSince1970: TimeInterval(share.untilS)).formatted(date: .omitted, time: .shortened)
        return String(localized: "Sharing your location (\(PrecisionText.name(share.precision))) until \(until)")
    }

    /// Before every radio send: what it costs us. A group message is flooded: relays add their
    /// own airtime, and nobody acknowledges it.
    private var isChannelChat: Bool {
        if case .channel = chat { return true }
        return false
    }

    private var costLine: String? {
        if chat.isPlain {
            let max = model.plainTextMax(chat)
            if plainOver > 0 { return String(localized: "\(plainOver) bytes too long: MeshCore takes \(max)") }
            guard let e = model.estimate(chat, text: draft) else {
                return isChannelChat ? String(localized: "Flooded to everyone on the channel, without acknowledgement")
                                     : String(localized: "Up to \(max) bytes, acknowledged by their radio")
            }
            let s = String(format: "%.1f", Double(e.airtimeMs) / 1000)
            return String(localized: "\(draft.utf8.count)/\(max) bytes · ≈ \(s) s of airtime")
        }
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
    /// Why the last try failed: the sheet and its photo stay (recheck R05).
    @State private var failure: String?

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
                            if let ui = DecodedImages.shared.image(data: image.full, maxPixel: 480) {
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
                    if let failure { Banner(kind: .sos, title: failure) }
                    Button(bridgeChosen ? String(localized: "Send the announcement") : String(localized: "Send the preview")) {
                        let e = if bridgeChosen, let original {
                            model.sendOverBridge(original, name: Self.name(of: original), to: chat,
                                                 anonymous: chosen == .tor)
                        } else {
                            model.sendImage(image, to: chat)
                        }
                        if let e { failure = e } else { done(nil) }
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
        // iroh runs while the sheet is open (SR-29).
        .onAppear { model.needBridge() }
        .onDisappear { model.releaseBridge() }
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

/// A file picked in the composer.
private struct FileDraft: Identifiable {
    let id = UUID()
    let name: String
    let data: Data
}

/// The SF Symbol for a file, by its extension.
private func fileSymbol(_ name: String) -> String {
    switch (name as NSString).pathExtension.lowercased() {
    case "pdf": "doc.richtext"
    case "txt", "md", "csv", "json": "doc.plaintext"
    case "gpx", "kml", "kmz", "geojson": "map"
    case "jpg", "jpeg", "png", "heic", "avif", "gif": "photo"
    case "zip", "gz", "7z", "tar": "doc.zipper"
    case "mp3", "m4a", "wav", "ogg": "waveform"
    default: "doc"
    }
}

/// The most a file can be: what the Internet bridge carries (§14.1, 64 MiB).
private let fileMax = 64 << 20

private struct PickProblem: Error {
    let message: String
}

/// A picked file, read in full only when it is at most `fileMax`: the size the system reports
/// is checked first, and reading stops one byte past the limit whatever it said (recheck R04).
private func readPicked(_ url: URL) -> Result<FileDraft, PickProblem> {
    let scoped = url.startAccessingSecurityScopedResource()
    defer { if scoped { url.stopAccessingSecurityScopedResource() } }
    let tooLarge = PickProblem(message: String(localized: "This file is larger than 64 MB, the most lichen can send."))
    if let size = try? url.resourceValues(forKeys: [.fileSizeKey]).fileSize, size > fileMax {
        return .failure(tooLarge)
    }
    guard let handle = try? FileHandle(forReadingFrom: url) else {
        return .failure(PickProblem(message: String(localized: "This file could not be read.")))
    }
    defer { try? handle.close() }
    guard let data = try? handle.read(upToCount: fileMax + 1), !data.isEmpty else {
        return .failure(PickProblem(message: String(localized: "This file could not be read.")))
    }
    if data.count > fileMax { return .failure(tooLarge) }
    return .success(FileDraft(name: url.lastPathComponent, data: data))
}

private func byteCount(_ n: Int) -> String {
    ByteCountFormatter.string(fromByteCount: Int64(n), countStyle: .file)
}

/// How to send a picked file (ADR-0022): over LoRa when small (up to 8 KB on the air once
/// compressed, ADR-0023; contacts only, its airtime shown), else over iroh or Tor; why not, when
/// neither can.
private struct SendFileSheet: View {
    let chat: ChatID
    let draft: FileDraft
    let done: (String?) -> Void
    @Environment(AppModel.self) private var model
    @State private var mode: Mode?
    /// The file as it would go over LoRa, compressed or not; nil when it does not fit.
    @State private var lora: LoraFile?
    @State private var checked = false
    /// Why the last try failed: the sheet and its file stay (recheck R05).
    @State private var failure: String?

    enum Mode { case lora, iroh, tor }

    private var isGroup: Bool {
        if case .group = chat { return true }
        return false
    }
    private var loraOK: Bool { !isGroup && lora != nil }
    private var irohOK: Bool { model.bridgeOnline }
    private var torOK: Bool { model.torStatus == .ready }
    /// The route picked, as long as it is still ready (recheck R05); else the best one ready.
    private var chosen: Mode? {
        switch mode {
        case .lora where loraOK: return .lora
        case .iroh where irohOK: return .iroh
        case .tor where torOK: return .tor
        default: break
        }
        if loraOK { return .lora }
        if irohOK { return .iroh }
        return torOK ? .tor : nil
    }
    private var loraDetail: String {
        if isGroup { return String(localized: "Contacts only: never flooded to a group") }
        guard checked else { return String(localized: "Direct, acknowledged") }
        guard let e = lora else {
            return String(localized: "Too large for LoRa (\(byteCount(draft.data.count)), up to 8 KB once compressed)")
        }
        let share = String(format: "%.0f", Double(e.airtimeMs) / 3_600)
        if e.compressed {
            return String(localized: "Compressed to \(byteCount(Int(e.wireBytes))) · \(fragmentsText(e.fragments)) · ≈ \(seconds(e.airtimeMs)) s of airtime, \(share) % of your hour")
        }
        return String(localized: "\(fragmentsText(e.fragments)) · ≈ \(seconds(e.airtimeMs)) s of airtime, \(share) % of your hour")
    }

    private var irohDetail: String {
        if irohOK { return String(localized: "1-packet LoRa announcement, peer-to-peer transfer") }
        if !model.irohEnabled { return String(localized: "iroh is off in Settings") }
        return String(localized: "Needs an Internet connection")
    }

    private var torDetail: String {
        switch model.torStatus {
        case .ready: return String(localized: "Anonymous, slower: they fetch it from your onion service")
        case .connecting, .publishing: return String(localized: "Connecting to Tor…")
        case .failed: return String(localized: "Tor could not connect")
        case .off: return String(localized: "Turn Tor on in Settings")
        }
    }

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 12) {
                    HStack(spacing: 14) {
                        Image(systemName: fileSymbol(draft.name))
                            .font(.system(size: 30))
                            .frame(width: 56, height: 56)
                            .background(Theme.s2, in: RoundedRectangle(cornerRadius: 12))
                        VStack(alignment: .leading, spacing: 3) {
                            Text(verbatim: draft.name).font(.system(.callout, weight: .semibold)).lineLimit(2)
                            Text(byteCount(draft.data.count)).font(.system(.footnote)).foregroundStyle(Theme.tx2)
                        }
                    }
                    SectionLabel("How to send it to \(model.title(of: chat))")
                    option(title: String(localized: "Over LoRa"), detail: loraDetail,
                           badge: lora.map { String(localized: "≈ \(seconds($0.airtimeMs)) s") } ?? "LoRa",
                           kind: .lora, selected: chosen == .lora, enabled: loraOK)
                        .onTapGesture { if loraOK { mode = .lora } }
                    option(title: String(localized: "Via iroh"), detail: irohDetail,
                           badge: String(localized: "1 packet"), kind: .iroh, selected: chosen == .iroh, enabled: irohOK)
                        .onTapGesture { if irohOK { mode = .iroh } }
                    option(title: String(localized: "Via Tor"), detail: torDetail,
                           badge: String(localized: "1 packet"), kind: .tor, selected: chosen == .tor, enabled: torOK)
                        .onTapGesture { if torOK { mode = .tor } }
                    if chosen == nil {
                        Banner(kind: .warn, title: String(localized: "This file cannot go over LoRa"),
                               message: String(localized: "Up to 8 KB fit in a LoRa message, once compressed. Turn on iroh or Tor in Settings to send larger files over the Internet."))
                    }
                    Text("Airtime left: \(seconds(model.airtimeLeftMs)) s. The recipient opens it with the app for its type.")
                        .font(.system(.footnote))
                        .foregroundStyle(Theme.tx2)
                    if let failure { Banner(kind: .sos, title: failure) }
                    Button(chosen == .lora ? String(localized: "Send over LoRa") : String(localized: "Send the announcement")) {
                        let e: String? = switch chosen {
                        case .lora: model.sendFile(draft.data, name: draft.name, to: chat)
                        case .iroh, .tor:
                            model.sendOverBridge(draft.data, name: draft.name, to: chat, anonymous: chosen == .tor, kind: 3)
                        case nil: nil
                        }
                        if let e { failure = e } else if chosen != nil { done(nil) }
                    }
                    .buttonStyle(.primary)
                    .disabled(chosen == nil)
                    .padding(.top, 8)
                }
                .padding(16)
            }
            .screen()
            .onAppear {
                if !checked {
                    lora = model.loraFile(chat, name: draft.name, data: draft.data)
                    checked = true
                }
            }
            .navigationTitle("Send a file")
            .navigationBarTitleDisplayMode(.inline)
            .toolbarBackground(Theme.s1, for: .navigationBar)
            .toolbarBackground(.visible, for: .navigationBar)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { done(nil) } }
            }
        }
        // iroh runs while the sheet is open (SR-29).
        .onAppear { model.needBridge() }
        .onDisappear { model.releaseBridge() }
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

/// A file in the conversation: its icon, name and size; tap to open it (QuickLook), long
/// press to share it. A small file came over LoRa; a larger one, see `BridgeSection`.
private struct FileCard: View {
    let message: StoredMessage
    @Environment(AppModel.self) private var model
    @State private var opened: URL?

    var body: some View {
        if case let .file(_, name, data) = message.message {
            FileRow(name: name, size: data.count, badge: TransportBadge(kind: .lora, label: "LoRa"))
                .padding(12)
                .frame(width: 250, alignment: .leading)
                .background(Theme.s1)
                .clipShape(RoundedRectangle(cornerRadius: 18))
                .overlay(RoundedRectangle(cornerRadius: 18).stroke(Theme.line))
                .contentShape(Rectangle())
                .onTapGesture { opened = model.openable(name: name, data: data) }
                .accessibilityAddTraits(.isButton)
                .accessibilityHint("Opens the file")
                .quickLookPreview($opened)
        }
    }
}

/// Icon, name and size of a file, and a transport badge.
private struct FileRow<Badge: View>: View {
    let name: String
    let size: Int
    let badge: Badge

    var body: some View {
        HStack(spacing: 12) {
            Image(systemName: fileSymbol(name))
                .font(.system(size: 22))
                .frame(width: 40, height: 40)
                .background(Theme.s2, in: RoundedRectangle(cornerRadius: 10))
            VStack(alignment: .leading, spacing: 3) {
                Text(verbatim: name).font(.system(.footnote, weight: .semibold)).lineLimit(2)
                HStack(spacing: 6) {
                    Text(byteCount(size)).font(.system(.caption)).foregroundStyle(Theme.tx2)
                    badge
                }
            }
            Spacer(minLength: 0)
        }
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
    @State private var opened: URL?

    var body: some View {
        if case let .announce(_, _, kind, hash, size, providers, name) = announce.message {
            let file = model.downloads[hash]
            let isFile = kind == 3
            let content = VStack(alignment: .leading, spacing: 8) {
                if isFile {
                    // A file (ADR-0022): opened with QuickLook once here.
                    FileRow(name: name, size: Int(size), badge: EmptyView())
                        .contentShape(Rectangle())
                        .onTapGesture { if let file { opened = model.openable(name: name, file: file) } }
                        .quickLookPreview($opened)
                } else if standalone, let file, let ui = DecodedImages.shared.image(url: file, maxPixel: 2048) {
                    Image(uiImage: ui).resizable().scaledToFit()
                        .clipShape(RoundedRectangle(cornerRadius: 12))
                        .onTapGesture { viewing = true }
                        .accessibilityAddTraits(.isButton)
                        .fullScreenCover(isPresented: $viewing) { PhotoViewer(message: announce) }
                }
                if !isFile {
                    HStack {
                        Text(verbatim: name).font(.system(.footnote, weight: .semibold)).lineLimit(1)
                        Spacer()
                        Text(ByteCountFormatter.string(fromByteCount: Int64(size), countStyle: .file))
                            .font(.system(.footnote)).foregroundStyle(Theme.tx2)
                    }
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
                    if isFile, let file {
                        Spacer()
                        Button("Open") { opened = model.openable(name: name, file: file) }
                            .font(.system(.footnote, weight: .semibold))
                            .frame(minHeight: 44)
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
                            Spacer()
                            Button("Cancel") { model.cancelFetch(announce) }
                                .font(.system(.footnote, weight: .semibold))
                                .frame(minHeight: 44)
                        }
                    } else {
                        let route = model.fetchRoute(providers)
                        if route.isEmpty {
                            // Offered, but nothing here reaches the sender.
                            Text(isFile ? String(localized: "Unavailable: turn on iroh or Tor in Settings to fetch the file.")
                                        : String(localized: "Unavailable: turn on iroh or Tor in Settings to fetch the original."))
                                .font(.system(.footnote)).foregroundStyle(Theme.tx2)
                        } else {
                            let torOnly = route.first?.transport == 2
                            Button(model.fetchErrors[hash] != nil ? String(localized: "Try again")
                                   : isFile ? String(localized: "Download")
                                   : String(localized: "Download in full resolution")) {
                                model.fetch(announce)
                            }
                            .buttonStyle(SecondaryButtonStyle(background: torOnly ? Theme.torBg : Theme.irohBg,
                                                              foreground: torOnly ? Theme.torFg : Theme.irohFg, height: 40))
                        }
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

/// In place of the composer for someone the user blocked (ADR-0034).
private struct BlockedBar: View {
    let name: String
    let unblock: () -> Void

    var body: some View {
        VStack(spacing: 10) {
            Text("You blocked \(name). Nothing they send reaches this phone.")
                .font(.system(.footnote))
                .foregroundStyle(Theme.tx2)
                .multilineTextAlignment(.center)
            Button("Unblock", action: unblock)
                .buttonStyle(SecondaryButtonStyle(height: 44))
        }
        .padding(14)
        .frame(maxWidth: .infinity)
        .background(Theme.s1)
    }
}

/// A message's text with its links and places tappable (ADR-0034): a link opens in the
/// browser when the user taps it (nothing is fetched before, no preview), a place opens
/// lichen's map. Only `http` and `https` addresses are links.
struct LinkedText: View {
    let text: String
    let outgoing: Bool
    @Environment(AppModel.self) private var model

    /// The scheme of the in-app links that stand for places; never registered with the system.
    private static let placeScheme = "lichen-place"

    var body: some View {
        Text(Self.attributed(text))
            .tint(outgoing ? Theme.actFg : Theme.tx)
            .environment(\.openURL, OpenURLAction { url in
                guard url.scheme == Self.placeScheme else {
                    return url.scheme == "https" || url.scheme == "http" ? .systemAction : .discarded
                }
                let parts = url.absoluteString.dropFirst(Self.placeScheme.count + 1).split(separator: ",")
                if parts.count == 2, let lat = Double(parts[0]), let lon = Double(parts[1]) {
                    model.showOnMap(lat: lat, lon: lon)
                }
                return .handled
            })
    }

    static func attributed(_ text: String) -> AttributedString {
        var a = AttributedString(text)
        for l in textLinks(text: text) {
            let range = NSRange(location: Int(l.start), length: Int(l.end - l.start))
            guard let r = Range(range, in: a) else { continue }
            switch l.target {
            case let .web(url):
                guard let u = URL(string: url) else { continue }
                a[r].link = u
            case let .place(lat, lon):
                a[r].link = URL(string: "\(placeScheme):\(lat),\(lon)")
            }
            a[r].underlineStyle = .single
        }
        return a
    }

    /// The places written in `text`.
    static func places(in text: String) -> [(lat: Double, lon: Double)] {
        textLinks(text: text).compactMap {
            if case let .place(lat, lon) = $0.target { return (lat, lon) }
            return nil
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

/// Keeps a scroll view on its last row while its content grows, as long as `pinned`; a scroll
/// by the user decides `pinned` again: at the end, or not. Before iOS 18, the scrolls at
/// opening only.
private struct FollowBottom: ViewModifier {
    @Binding var pinned: Bool
    let toBottom: () -> Void

    private struct Geometry: Equatable {
        var content: CGFloat
        var visibleEnd: CGFloat
    }

    func body(content: Content) -> some View {
        if #available(iOS 18, *) {
            content.onScrollGeometryChange(for: Geometry.self) {
                Geometry(content: $0.contentSize.height, visibleEnd: $0.visibleRect.maxY)
            } action: { old, new in
                if new.content != old.content {
                    if pinned { toBottom() }
                } else {
                    pinned = new.visibleEnd >= new.content - 32
                }
            }
        } else {
            content
        }
    }
}
