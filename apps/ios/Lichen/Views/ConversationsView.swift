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
    @State private var showingArchived = false

    var body: some View {
        NavigationStack(path: $path) {
            List {
                Section {
                    header
                    // The radio in one line; its card (airtime, transports) a tap away, or always
                    // with Settings › Show radio details.
                    RadioLine()
                    StatusBanners()
                    ForEach(model.invitations, id: \.gid) { InvitationCard(invitation: $0) }
                    ForEach(model.requests, id: \.identity) { RequestCard(request: $0) }
                    if rows.isEmpty && archivedRows.isEmpty {
                        EmptyConversations { addingContact = true }
                    }
                }
                .listRowBackground(Color.clear)
                .listRowSeparator(.hidden)
                .listRowInsets(EdgeInsets(top: 6, leading: 0, bottom: 6, trailing: 0))
                if !rows.isEmpty {
                    Section {
                        ForEach(rows) { chatRow($0) }
                    }
                    .listRowSeparatorTint(Theme.line)
                }
                // Archived conversations (ADR-0034): one row, opened in place.
                if !archivedRows.isEmpty {
                    Section {
                        Button { withAnimation { showingArchived.toggle() } } label: {
                            HStack(spacing: 12) {
                                Image(systemName: "archivebox")
                                    .frame(width: 44)
                                    .foregroundStyle(Theme.tx2)
                                Text("Archived").font(.system(.callout, weight: .semibold))
                                Spacer()
                                Text(String(archivedRows.count)).foregroundStyle(Theme.tx2)
                                Image(systemName: showingArchived ? "chevron.up" : "chevron.down")
                                    .font(.system(.footnote, weight: .semibold))
                                    .foregroundStyle(Theme.tx2)
                            }
                            .padding(.horizontal, 16)
                            .frame(minHeight: 52)
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                        .listRowInsets(EdgeInsets())
                        .listRowBackground(Theme.s1)
                        .accessibilityLabel(String(localized: "Archived, \(archivedRows.count)"))
                        .accessibilityHint(showingArchived ? String(localized: "Hides them") : String(localized: "Shows them"))
                        if showingArchived {
                            ForEach(archivedRows) { chatRow($0) }
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
            // One view per conversation: a path that switches chats (a notification, a debug
            // retarget) starts the new one afresh, with its own history, draft and state.
            .navigationDestination(for: ChatID.self) { ChatView(chat: $0).id($0) }
            .sheet(isPresented: $searching) {
                SearchMessagesView { chat, id in
                    searching = false
                    if let id { model.focus = (chat, id) }
                    path = [chat]
                }
            }
            // A conversation asked for elsewhere: opened once, then the request is gone, so that
            // coming back to this list stays here.
            .onChange(of: model.openRequest) {
                if let chat = model.openRequest {
                    path = [chat]
                    model.openRequest = nil
                }
            }
            #if DEBUG
            // -screen join: the scanner opens on the demo's invitation.
            .onChange(of: model.debugInvite) { if model.debugInvite != nil { addingContact = true } }
            #endif
            .onAppear {
                // A chat asked for before this list appeared (SOS reply, debug `-open`).
                if let chat = model.openRequest {
                    path = [chat]
                    model.openRequest = nil
                }
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
            .sheet(isPresented: Binding(get: { model.showPairing }, set: { model.showPairing = $0 })) {
                NavigationStack {
                    PairingView()
                        .toolbar {
                            ToolbarItem(placement: .cancellationAction) { Button("Close") { model.showPairing = false } }
                        }
                }
            }
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

    /// A conversation in the list: open it; swipe right to pin, left to archive, delete or
    /// rename a contact; long press for the rest.
    private func chatRow(_ row: Row) -> some View {
        Button { path.append(row.id) } label: { ChatRow(row: row) }
            .buttonStyle(.plain)
            .listRowInsets(EdgeInsets())
            .listRowBackground(Theme.s1)
            .alignmentGuide(.listRowSeparatorLeading) { _ in 72 }
            // Swipe right: pin; left: delete (confirmed below), archive, rename a contact.
            .swipeActions(edge: .leading, allowsFullSwipe: true) {
                Button { model.setPinned(row.id, !row.pinned) } label: {
                    Label(row.pinned ? "Unpin" : "Pin", systemImage: row.pinned ? "pin.slash" : "pin")
                }
                .tint(Theme.tx2)
            }
            .swipeActions(edge: .trailing, allowsFullSwipe: false) {
                Button { deleting = row } label: { Label("Delete", systemImage: "trash") }
                    .tint(Theme.sos)
                Button { model.setArchived(row.id, !row.archived) } label: {
                    Label(row.archived ? "Unarchive" : "Archive",
                          systemImage: row.archived ? "tray.and.arrow.up" : "archivebox")
                }
                .tint(Theme.tx2)
                if let key = renamable(row) {
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
                Button { model.setArchived(row.id, !row.archived) } label: {
                    Label(row.archived ? "Unarchive" : "Archive",
                          systemImage: row.archived ? "tray.and.arrow.up" : "archivebox")
                }
                if let key = renamable(row) {
                    Button { renaming = key } label: { Label("Rename", systemImage: "pencil") }
                }
                Button(role: .destructive) { deleting = row } label: {
                    Label("Delete conversation", systemImage: "trash")
                }
            }
    }

    /// The node a row's conversation is with, when the core keeps it as a contact (people or
    /// not): only that can be renamed. A room is not renamed here.
    private func renamable(_ row: Row) -> Data? {
        guard let key = model.node(of: row.id), model.coreContacts.contains(where: { $0.key == key }) else { return nil }
        return key
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
        var archived = false
        var pinned: Bool { pinnedAt != nil }
    }

    /// The list: what is not archived.
    private var rows: [Row] { allRows.filter { !$0.archived } }
    private var archivedRows: [Row] { allRows.filter(\.archived) }

    private var allRows: [Row] {
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
        // Every other conversation once it has a message, whatever the radio's contact list
        // says (§15.8): lichen and MeshCore chats, room servers, and a group or a channel the
        // radio no longer has. Radios the node heard but nobody wrote to stay in "New message".
        let listed = Set(out.map(\.id))
        for conv in model.conversations where !listed.contains(conv.chat) {
            let chat = conv.chat
            out.append(Row(
                id: chat, title: model.conversationTitle(chat, named: conv.name), key: chat.bytes,
                preview: conv.last.preview(model), date: conv.last.date, badge: badge(chat),
                unread: Int(conv.unread), waiting: conv.last.isWaiting,
                pinnedAt: model.prefs[chat]?.pinnedAtMs, muted: model.isMuted(chat)
            ))
        }
        for i in out.indices { out[i].archived = model.isArchived(out[i].id) }
        // Pinned first (the latest pinned on top), then the most recent.
        return out.sorted {
            if $0.pinnedAt != $1.pinnedAt { return ($0.pinnedAt ?? .min) > ($1.pinnedAt ?? .min) }
            return ($0.date ?? .distantPast, $1.title) > ($1.date ?? .distantPast, $0.title)
        }
    }

    /// How a conversation goes: over LoRa (with the contact's hops when known), MeshCore.
    private func badge(_ chat: ChatID) -> String {
        switch chat {
        case let .contact(key): hopsBadge(model.contacts.first { $0.key == key }?.hops)
        case .group: String(localized: "Group · flood")
        case .mesh: String(localized: "MeshCore")
        case .channel: String(localized: "MeshCore channel")
        case .room: String(localized: "MeshCore room")
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
                        Image(systemName: "pin.fill").font(.system(.caption)).foregroundStyle(Theme.tx2)
                            .accessibilityLabel("Pinned")
                    }
                    if row.muted {
                        Image(systemName: "bell.slash.fill").font(.system(.caption)).foregroundStyle(Theme.tx2)
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

/// The radio's state in one line, under the title; a tap opens its card (the airtime left, the
/// transports) and another closes it. With Settings › Show radio details: the card.
struct RadioLine: View {
    @Environment(AppModel.self) private var model
    @AppStorage(radioDetailsKey) private var details = false
    @State private var open = false

    var body: some View {
        if details {
            RadioCard()
        } else if open {
            Button { withAnimation(.snappy) { open = false } } label: { RadioCard(folds: true) }
                .buttonStyle(.plain)
                .accessibilityHint("Hides the radio's details")
        } else {
            Button { withAnimation(.snappy) { open = true } } label: {
                HStack(spacing: 8) {
                    Circle().fill(model.isConnected ? Theme.ok : Theme.tx2).frame(width: 8, height: 8)
                    Text(radioHeadline(model))
                        .font(.system(.subheadline))
                        .foregroundStyle(Theme.tx2)
                        .multilineTextAlignment(.leading)
                    Spacer()
                    Image(systemName: "chevron.down")
                        .font(.system(.footnote, weight: .semibold))
                        .foregroundStyle(Theme.tx2)
                }
                .padding(.horizontal, 4)
                .frame(minHeight: 44)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityHint("Shows the airtime left and the transports")
            .accessibilityIdentifier("radioLine")
        }
    }
}

/// "Demo radio connected", "No radio connected"…
@MainActor func radioHeadline(_ model: AppModel) -> String {
    switch model.companionState {
    case let .connected(name): return String(localized: "\(name) connected")
    case let .connecting(name): return String(localized: "Connecting to \(name)…")
    case .scanning: return String(localized: "Looking for nodes…")
    case .off: return String(localized: "Bluetooth is off")
    case .idle: return String(localized: "No radio connected")
    }
}

/// Fragments, airtime and signal on every message, and the radio card on Messages (Settings ›
/// Show radio details); off, they wait behind long press › Info and a tap on the radio line.
let radioDetailsKey = "lichen.radioDetails"

/// Radio, airtime left this hour, transports (ADR-0007); `folds`: a tap folds it back into its
/// line.
struct RadioCard: View {
    var folds = false
    @Environment(AppModel.self) private var model

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(alignment: .firstTextBaseline) {
                Circle().fill(model.isConnected ? Theme.ok : Theme.tx2).frame(width: 8, height: 8)
                // The radio's name whole, its link and parameters under it.
                VStack(alignment: .leading, spacing: 2) {
                    Text(radioHeadline(model)).font(.system(.subheadline, weight: .semibold))
                    if let r = model.radio, model.isConnected {
                        // How the radio is reached: Bluetooth, TCP, or the demo's simulation.
                        let link = model.isDemo ? String(localized: "Demo") : model.link == .tcp ? "TCP" : "BLE"
                        Text("\(link) · SF\(r.sf) / \(bandwidth(r.bwHz))")
                            .font(.system(.footnote))
                            .foregroundStyle(Theme.tx2)
                    }
                }
                Spacer()
                if folds {
                    Image(systemName: "chevron.up")
                        .font(.system(.footnote, weight: .semibold))
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
                        .font(.system(.caption))
                        .foregroundStyle(Theme.tx2)
                        .lineLimit(1)
                }
            }
        }
        .card(padding: 16)
        .contentShape(Rectangle())
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

/// Someone using a contact's radio, with an identity it does not have, wants to write
/// (protocol.md §4.6, ADR-0046): what they sent, as text lines (nothing decoded or fetched),
/// and Accept, Block or Delete.
struct RequestCard: View {
    let request: IdentityRequest
    @Environment(AppModel.self) private var model

    /// The last lines shown; the earlier ones are counted.
    private static let shown = 3

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(alignment: .top, spacing: 10) {
                Image(systemName: "person.crop.circle.badge.questionmark")
                    .font(.system(.footnote, weight: .bold))
                    .foregroundStyle(Theme.tx2)
                VStack(alignment: .leading, spacing: 3) {
                    Text("Someone using \(request.name)'s radio wants to write to you")
                        .font(.system(.subheadline, weight: .semibold))
                        .foregroundStyle(Theme.tx)
                        .fixedSize(horizontal: false, vertical: true)
                    Text("Anyone in range can write under a radio's name. Accept only if you know who this is; accepting does not verify them.")
                        .font(.system(.footnote)).foregroundStyle(Theme.tx2)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
            if request.messages.count > Self.shown {
                Text("Earlier messages: \(request.messages.count - Self.shown)")
                    .font(.system(.footnote)).foregroundStyle(Theme.tx2)
            }
            ForEach(Array(request.messages.suffix(Self.shown).enumerated()), id: \.offset) { _, line in
                Text(line.message.display)
                    .font(.system(.callout))
                    .foregroundStyle(Theme.tx)
                    .lineLimit(4)
                    .padding(.horizontal, 10)
                    .padding(.vertical, 6)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(Theme.s2, in: RoundedRectangle(cornerRadius: 10))
            }
            HStack(spacing: 10) {
                // Each its own tap only: they share a List row.
                Button { model.acceptRequest(request.identity) } label: {
                    Text("Accept").frame(maxWidth: .infinity, minHeight: 44)
                }
                .buttonStyle(.plain)
                .font(.system(.subheadline, weight: .semibold))
                .background(Theme.actBg, in: RoundedRectangle(cornerRadius: 10))
                .foregroundStyle(Theme.actFg)
                Button { model.blockRequest(request.identity) } label: {
                    Text("Block").frame(maxWidth: .infinity, minHeight: 44)
                }
                .buttonStyle(.plain)
                .font(.system(.subheadline, weight: .semibold))
                .background(Theme.s2, in: RoundedRectangle(cornerRadius: 10))
                .foregroundStyle(Theme.tx)
                Button { model.deleteRequest(request.identity) } label: {
                    Text("Delete").frame(maxWidth: .infinity, minHeight: 44)
                }
                .buttonStyle(.plain)
                .font(.system(.subheadline, weight: .semibold))
                .background(Theme.s2, in: RoundedRectangle(cornerRadius: 10))
                .foregroundStyle(Theme.tx)
            }
        }
        .padding(12)
        .background(Theme.s1, in: RoundedRectangle(cornerRadius: 14))
        .accessibilityElement(children: .contain)
    }
}

/// Degraded states that concern the whole app (mockup `etats`): radio, budget.
struct StatusBanners: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        Group {
            // No radio was ever paired on this phone (D-1): not one that dropped.
            if !model.isConnected && !model.isDemo && model.savedRadios.isEmpty {
                Banner(
                    kind: .warn,
                    title: String(localized: "No radio connected yet"),
                    message: String(localized: "Connect your radio to send and receive."),
                    action: (String(localized: "Connect"), { model.showPairing = true })
                )
            } else if !model.isConnected && (!model.isDemo || model.demoRadioOff) {
                // The demo's simulated radio switched off (Settings › Demo, `-offline`) too: Retry
                // switches it back on.
                Banner(
                    kind: .warn,
                    title: String(localized: "Radio disconnected"),
                    message: model.companionState == .off
                        ? String(localized: "Turn Bluetooth on to reach your radio.")
                        : String(localized: "Messages wait until the radio is back."),
                    action: (String(localized: "Retry"), { model.isDemo ? model.setDemoRadio(on: true) : model.retryRadio() })
                )
            }
            // A contact speaks a newer protocol version (ADR-0029).
            if !model.newerPeers.isEmpty {
                let names = ListFormatter.localizedString(byJoining: model.newerPeers.map { model.name(of: $0) }.sorted())
                Banner(kind: .warn, title: String(localized: "Update lichen"),
                       message: String(localized: "A newer version is in use by \(names). Some of what they send may not show."))
            }
            // Someone is searching for us (protocol.md §11): our phone answers their pings.
            if let first = model.searchedBy.first {
                let names = ListFormatter.localizedString(byJoining: Array(Set(model.searchedBy.map { model.name(of: $0.by) })).sorted())
                Banner(
                    kind: .sos,
                    title: String(localized: "\(names) is locating you"),
                    message: model.loweredPower.map { String(localized: "Your radio transmits at \($0) dBm while locating, then goes back.") }
                        ?? String(localized: "Your phone answers their pings: SOS, or you let the group locate you."),
                    action: (String(localized: "Open"), { model.openRequest = .group(gid: first.gid) })
                )
            }
            // The radio's battery (ADR-0042).
            if model.radioBatteryLow, let pct = model.radioSettings?.batteryPercent {
                Banner(
                    kind: .warn,
                    title: String(localized: "Radio battery low: \(Int(pct)) %"),
                    message: String(localized: "Charge it soon: messages wait while it is off.")
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

