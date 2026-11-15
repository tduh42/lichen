import SwiftUI

/// "Messages": radio, airtime left, then every conversation (mockup `conversations`).
struct ConversationsView: View {
    @Environment(AppModel.self) private var model
    @State private var composing = false
    @State private var addingContact = false
    @State private var path: [ChatID] = []
    @State private var deleting: Row?
    @State private var renaming: Data?
    @State private var searching = false

    var body: some View {
        NavigationStack(path: $path) {
            List {
                Section {
                    header
                    RadioCard()
                    StatusBanners()
                    ForEach(model.invitations, id: \.gid) { InvitationCard(invitation: $0) }
                    if rows.isEmpty {
                        EmptyConversations { addingContact = true }
                    }
                }
                .listRowBackground(Color.clear)
                .listRowSeparator(.hidden)
                .listRowInsets(EdgeInsets(top: 6, leading: 0, bottom: 6, trailing: 0))
                if !rows.isEmpty {
                    Section {
                        ForEach(rows) { row in
                            Button { path.append(row.id) } label: { ChatRow(row: row) }
                                .buttonStyle(.plain)
                                .listRowInsets(EdgeInsets())
                                .listRowBackground(Theme.s1)
                                .alignmentGuide(.listRowSeparatorLeading) { _ in 72 }
                                // Swipe right: pin; left: delete (confirmed below), rename a contact.
                                .swipeActions(edge: .leading, allowsFullSwipe: true) {
                                    Button { model.setPinned(row.id, !row.pinned) } label: {
                                        Label(row.pinned ? "Unpin" : "Pin", systemImage: row.pinned ? "pin.slash" : "pin")
                                    }
                                    .tint(Theme.tx2)
                                }
                                .swipeActions(edge: .trailing, allowsFullSwipe: false) {
                                    Button { deleting = row } label: { Label("Delete", systemImage: "trash") }
                                        .tint(Theme.sos)
                                    if case let .contact(key) = row.id {
                                        Button { renaming = key } label: { Label("Rename", systemImage: "pencil") }
                                            .tint(Theme.tx2)
                                    }
                                }
                                .contextMenu {
                                    Button { model.setPinned(row.id, !row.pinned) } label: {
                                        Label(row.pinned ? "Unpin" : "Pin", systemImage: row.pinned ? "pin.slash" : "pin")
                                    }
                                    if row.muted {
                                        Button { model.unmute(row.id) } label: { Label("Unmute", systemImage: "bell") }
                                    } else {
                                        // SOS alerts are never muted.
                                        Menu {
                                            Button("For 1 hour") { model.mute(row.id, hours: 1) }
                                            Button("For 8 hours") { model.mute(row.id, hours: 8) }
                                            Button("Until I turn it back on") { model.mute(row.id, hours: nil) }
                                        } label: {
                                            Label("Mute", systemImage: "bell.slash")
                                        }
                                    }
                                    if case let .contact(key) = row.id {
                                        Button { renaming = key } label: { Label("Rename", systemImage: "pencil") }
                                    }
                                    Button(role: .destructive) { deleting = row } label: {
                                        Label("Delete conversation", systemImage: "trash")
                                    }
                                }
                        }
                    }
                    .listRowSeparatorTint(Theme.line)
                }
            }
            .listStyle(.insetGrouped)
            .listSectionSpacing(12)
            .scrollContentBackground(.hidden)
            .background(Theme.bg)
            .toolbar(.hidden, for: .navigationBar)
            .navigationDestination(for: ChatID.self) { ChatView(chat: $0) }
            .sheet(isPresented: $searching) {
                SearchMessagesView { chat, id in
                    searching = false
                    if let id { model.focus = (chat, id) }
                    path = [chat]
                }
            }
            .onChange(of: model.debugOpen) {
                if let chat = model.debugOpen { path = [chat] }
            }
            #if DEBUG
            // -screen join: the scanner opens on the demo's invitation.
            .onChange(of: model.debugInvite) { if model.debugInvite != nil { addingContact = true } }
            #endif
            .onAppear {
                // A chat asked for before this list appeared (SOS reply, debug `-open`).
                if let chat = model.debugOpen, path.isEmpty { path = [chat] }
                #if DEBUG
                if model.debugScreen == "contact" { addingContact = true }
                // -screen searchMessages -query <text>: the search sheet, filled in.
                if model.debugScreen == "searchMessages" { searching = true }
                #endif
            }
            .sheet(isPresented: $composing) {
                NewChatSheet { chat in
                    composing = false
                    path.append(chat)
                }
            }
            .sheet(isPresented: $addingContact) { NavigationStack { AddContactView() } }
            .renameAlert(node: renaming, isPresented: Binding(get: { renaming != nil }, set: { if !$0 { renaming = nil } }))
            .confirmationDialog(
                deleting.map { String(localized: "Delete the conversation with \($0.title)?") } ?? "",
                isPresented: Binding(get: { deleting != nil }, set: { if !$0 { deleting = nil } }),
                titleVisibility: .visible
            ) {
                Button("Delete", role: .destructive) {
                    if let row = deleting { model.deleteConversation(row.id) }
                    deleting = nil
                }
            } message: {
                Text("Its messages are deleted from this phone. The contact stays.")
            }
        }
    }

    private var header: some View {
        HStack(alignment: .bottom) {
            Text("Messages")
                .font(.system(.largeTitle, weight: .bold))
                .tracking(-0.5)
            Spacer()
            HStack(spacing: 8) {
                RoundIconButton(systemName: "magnifyingglass", label: "Search messages") { searching = true }
                RoundIconButton(systemName: "qrcode", label: "Add a contact") { addingContact = true }
                RoundIconButton(systemName: "plus", label: "New message", prominent: true) { composing = true }
            }
        }
        .padding(.top, 8)
        .padding(.horizontal, 4)
    }

    struct Row: Identifiable {
        let id: ChatID
        let title: String
        let key: Data
        let preview: String
        let date: Date?
        let badge: String
        let unread: Int
        /// Our last message is still waiting (§6.1).
        var waiting = false
        /// Pinned (since when, Unix ms) and muted now.
        var pinnedAt: Int64?
        var muted = false
        var pinned: Bool { pinnedAt != nil }
    }

    private var rows: [Row] {
        var out: [Row] = []
        for g in model.groups {
            let chat = ChatID.group(gid: g.gid)
            let last = model.last(chat)
            out.append(Row(
                id: chat, title: g.name, key: g.gid,
                preview: last?.preview(model) ?? String(localized: "\(g.members.count) members"),
                date: last?.date, badge: String(localized: "Group · flood"), unread: model.unread(chat),
                waiting: last?.isWaiting ?? false,
                pinnedAt: model.prefs[chat]?.pinnedAtMs, muted: model.isMuted(chat)
            ))
        }
        // A contact appears once there is something to show: radios the node heard but nobody
        // wrote to stay in "New message".
        for c in model.contacts {
            let chat = ChatID.contact(key: c.key)
            guard let last = model.last(chat) else { continue }
            out.append(Row(
                id: chat, title: c.name, key: c.key, preview: last.preview(model),
                date: last.date, badge: hopsBadge(c.hops), unread: model.unread(chat),
                waiting: last.isWaiting,
                pinnedAt: model.prefs[chat]?.pinnedAtMs, muted: model.isMuted(chat)
            ))
        }
        // MeshCore chats (§15): a node someone wrote to in plain, and the radio's channels.
        for c in model.contacts {
            let chat = ChatID.mesh(key: c.key)
            guard let last = model.last(chat) else { continue }
            out.append(Row(
                id: chat, title: c.name, key: c.key, preview: last.preview(model),
                date: last.date, badge: String(localized: "MeshCore"), unread: model.unread(chat),
                waiting: last.isWaiting,
                pinnedAt: model.prefs[chat]?.pinnedAtMs, muted: model.isMuted(chat)
            ))
        }
        for ch in model.channels {
            let chat = ChatID.channel(id: ch.id)
            let last = model.last(chat)
            out.append(Row(
                id: chat, title: ch.name, key: ch.id,
                preview: last?.preview(model) ?? ch.kind.label,
                date: last?.date, badge: String(localized: "MeshCore channel"), unread: model.unread(chat),
                waiting: last?.isWaiting ?? false,
                pinnedAt: model.prefs[chat]?.pinnedAtMs, muted: model.isMuted(chat)
            ))
        }
        // Pinned first (the latest pinned on top), then the most recent.
        return out.sorted {
            if $0.pinnedAt != $1.pinnedAt { return ($0.pinnedAt ?? .min) > ($1.pinnedAt ?? .min) }
            return ($0.date ?? .distantPast, $1.title) > ($1.date ?? .distantPast, $0.title)
        }
    }
}

func hopsBadge(_ hops: UInt8?) -> String {
    switch hops {
    case nil: return String(localized: "LoRa")
    case 0: return String(localized: "LoRa · direct")
    case let n?: return String(localized: "LoRa · \(Int(n)) hops")
    }
}

private struct ChatRow: View {
    let row: ConversationsView.Row

    var body: some View {
        HStack(alignment: .top, spacing: 12) {
            Avatar(name: row.title, key: row.key)
            VStack(alignment: .leading, spacing: 3) {
                HStack(alignment: .firstTextBaseline, spacing: 6) {
                    Text(row.title).font(.system(.callout, weight: .semibold)).lineLimit(1)
                    if row.pinned {
                        Image(systemName: "pin.fill").font(.system(.caption2)).foregroundStyle(Theme.tx2)
                            .accessibilityLabel("Pinned")
                    }
                    if row.muted {
                        Image(systemName: "bell.slash.fill").font(.system(.caption2)).foregroundStyle(Theme.tx2)
                            .accessibilityLabel("Muted")
                    }
                    Spacer()
                    if let d = row.date {
                        Text(d, style: .time).font(.system(.footnote)).foregroundStyle(Theme.tx2)
                    }
                }
                if !row.preview.isEmpty {
                    HStack(spacing: 4) {
                        if row.waiting {
                            Image(systemName: "clock").accessibilityLabel("Waiting to be sent")
                        }
                        Text(row.preview).lineLimit(1)
                    }
                    .font(.system(.subheadline))
                    .foregroundStyle(Theme.tx2)
                }
                HStack {
                    // A MeshCore chat is marked as such (§15.6), never in LoRa orange alone.
                    TransportBadge(kind: row.id.isPlain ? .neutral : .lora, label: row.badge)
                    Spacer()
                    if row.unread > 0 {
                        Text(String(row.unread))
                            .font(.system(.caption, weight: .bold))
                            .padding(.horizontal, 6)
                            .frame(minWidth: 20, minHeight: 20)
                            // Grey when muted.
                            .background(row.muted ? Theme.s2 : Theme.actBg, in: Capsule())
                            .foregroundStyle(row.muted ? Theme.tx2 : Theme.actFg)
                            .accessibilityLabel("\(row.unread) unread")
                    }
                }
            }
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 12)
        .contentShape(Rectangle())
    }
}

/// Radio, airtime left this hour, transports (ADR-0007).
struct RadioCard: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack {
                Circle().fill(model.isConnected ? Theme.ok : Theme.tx2).frame(width: 8, height: 8)
                Text(headline).font(.system(.subheadline, weight: .semibold)).lineLimit(1)
                Spacer()
                if let r = model.radio, model.isConnected {
                    Text("BLE · SF\(r.sf) / \(bandwidth(r.bwHz))")
                        .font(.system(.footnote))
                        .foregroundStyle(Theme.tx2)
                }
            }
            AirtimeMeter(usedMs: model.budget.usedMs, limitMs: model.budget.limitMs)
            HStack(spacing: 6) {
                TransportBadge(kind: .lora, label: "LoRa")
                Text(model.isConnected ? "available" : "unavailable")
                    .font(.system(.caption))
                    .foregroundStyle(Theme.tx2)
                Spacer()
                if model.bridgeOnline { TransportBadge(kind: .iroh, label: "iroh") }
                if model.torStatus == .ready { TransportBadge(kind: .tor, label: "Tor") }
                if !model.bridgeOnline && model.torStatus != .ready {
                    Text("No Internet bridge")
                        .font(.system(.caption2))
                        .foregroundStyle(Theme.tx2)
                        .lineLimit(1)
                }
            }
        }
        .card(padding: 16)
    }

    private var headline: String {
        switch model.companionState {
        case let .connected(name): return String(localized: "\(name) connected")
        case let .connecting(name): return String(localized: "Connecting to \(name)…")
        case .scanning: return String(localized: "Looking for nodes…")
        case .off: return String(localized: "Bluetooth is off")
        case .idle: return String(localized: "No radio connected")
        }
    }
}

/// A group that lists us, from someone not verified in person: joined only if the user says
/// so (§10.1).
struct InvitationCard: View {
    let invitation: GroupInvitation
    @Environment(AppModel.self) private var model

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(spacing: 10) {
                Image(systemName: "person.3")
                    .font(.system(.footnote, weight: .bold))
                    .foregroundStyle(Theme.tx2)
                VStack(alignment: .leading, spacing: 3) {
                    Text("\(invitation.adminName) invites you to “\(invitation.name)”")
                        .font(.system(.subheadline, weight: .semibold))
                        .foregroundStyle(Theme.tx)
                    Text(members)
                        .font(.system(.footnote)).foregroundStyle(Theme.tx2)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
            HStack(spacing: 10) {
                // Each its own tap only: they share a List row.
                Button { model.acceptGroup(invitation.gid) } label: {
                    Text("Join").frame(maxWidth: .infinity, minHeight: 44)
                }
                .buttonStyle(.plain)
                .font(.system(.subheadline, weight: .semibold))
                .background(Theme.actBg, in: RoundedRectangle(cornerRadius: 10))
                .foregroundStyle(Theme.actFg)
                Button { model.declineGroup(invitation.gid) } label: {
                    Text("Decline").frame(maxWidth: .infinity, minHeight: 44)
                }
                .buttonStyle(.plain)
                .font(.system(.subheadline, weight: .semibold))
                .background(Theme.s2, in: RoundedRectangle(cornerRadius: 10))
                .foregroundStyle(Theme.tx)
            }
        }
        .padding(12)
        .background(Theme.s1, in: RoundedRectangle(cornerRadius: 14))
    }

    private var members: String {
        let others = invitation.members
        if others.isEmpty {
            return String(localized: "Nobody else yet. You did not verify this person in person: join only if you know them.")
        }
        let list = ListFormatter.localizedString(byJoining: others)
        return String(localized: "With \(list). You did not verify this person in person: join only if you know them.")
    }
}

/// Degraded states that concern the whole app (mockup `etats`): radio, budget.
struct StatusBanners: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        Group {
            if !model.isConnected && !model.isDemo {
                Banner(
                    kind: .warn,
                    title: String(localized: "Radio disconnected"),
                    message: model.companionState == .off
                        ? String(localized: "Turn Bluetooth on to reach your radio.")
                        : String(localized: "Messages wait until the radio is back."),
                    action: (String(localized: "Retry"), { model.companion.scan() })
                )
            }
            // Someone is searching for us (protocol.md §11): our phone answers their pings.
            if let first = model.searchedBy.first {
                let names = ListFormatter.localizedString(byJoining: Array(Set(model.searchedBy.map { model.name(of: $0.by) })).sorted())
                Banner(
                    kind: .sos,
                    title: String(localized: "\(names) is searching for you"),
                    message: model.loweredPower.map { String(localized: "Your radio transmits at \($0) dBm for the search, then goes back.") }
                        ?? String(localized: "Your phone answers their pings: SOS, or you let the group locate you."),
                    action: (String(localized: "Open"), { model.debugOpen = .group(gid: first.gid) })
                )
            }
            if model.isConnected && model.airtimeLeftMs < 10_000 {
                Banner(
                    kind: .lora,
                    title: String(localized: "Airtime budget spent"),
                    message: String(localized: "Short messages go first; longer sends wait for the hour to free up.")
                )
            }
        }
    }
}

/// No conversation yet (mockup `etats`, bottom).
private struct EmptyConversations: View {
    let addContact: () -> Void

    var body: some View {
        VStack(spacing: 12) {
            ChirpMark(color: Theme.tx2).frame(width: 40, height: 40)
            Text("No conversations yet").font(.system(.body, weight: .semibold))
            Text("Add a contact by scanning their code, in person.")
                .font(.system(.subheadline))
                .foregroundStyle(Theme.tx2)
                .multilineTextAlignment(.center)
            Button("Add a contact", action: addContact)
                .font(.system(.subheadline, weight: .semibold))
                .padding(.horizontal, 18)
                .frame(minHeight: 44)
                .background(Theme.actBg, in: RoundedRectangle(cornerRadius: 12))
                .foregroundStyle(Theme.actFg)
        }
        .frame(maxWidth: .infinity)
        .padding(24)
        .overlay(RoundedRectangle(cornerRadius: 14).stroke(Theme.line, style: StrokeStyle(lineWidth: 1, dash: [4, 4])))
        .padding(.top, 8)
    }
}

