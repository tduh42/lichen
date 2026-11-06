import SwiftUI

/// "Messages": radio, airtime left, then every conversation (mockup `conversations`).
struct ConversationsView: View {
    @Environment(AppModel.self) private var model
    @State private var composing = false
    @State private var addingContact = false
    @State private var path: [ChatID] = []

    var body: some View {
        NavigationStack(path: $path) {
            ScrollView {
                VStack(spacing: 12) {
                    header
                    RadioCard()
                    StatusBanners()
                    if rows.isEmpty {
                        EmptyConversations { addingContact = true }
                    } else {
                        VStack(spacing: 0) {
                            ForEach(Array(rows.enumerated()), id: \.element.id) { i, row in
                                if i > 0 { CardDivider(inset: 72) }
                                NavigationLink(value: row.id) { ChatRow(row: row) }
                                    .buttonStyle(.plain)
                            }
                        }
                        .background(Theme.s1, in: RoundedRectangle(cornerRadius: 14))
                    }
                }
                .padding(.horizontal, 16)
                .padding(.bottom, 16)
            }
            .screen()
            .toolbar(.hidden, for: .navigationBar)
            .navigationDestination(for: ChatID.self) { ChatView(chat: $0) }
            .onChange(of: model.debugOpen) {
                if let chat = model.debugOpen { path = [chat] }
            }
            .onAppear {
                // A chat asked for before this list appeared (SOS reply, debug `-open`).
                if let chat = model.debugOpen, path.isEmpty { path = [chat] }
                #if DEBUG
                if model.debugScreen == "contact" { addingContact = true }
                #endif
            }
            .sheet(isPresented: $composing) {
                NewChatSheet { chat in
                    composing = false
                    path.append(chat)
                }
            }
            .sheet(isPresented: $addingContact) { NavigationStack { AddContactView() } }
        }
    }

    private var header: some View {
        HStack(alignment: .bottom) {
            Text("Messages")
                .font(.system(size: 34, weight: .bold))
                .tracking(-0.5)
            Spacer()
            HStack(spacing: 8) {
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
    }

    private var rows: [Row] {
        var out: [Row] = []
        for g in model.groups {
            let chat = ChatID.group(gid: g.gid)
            let last = model.last(chat)
            out.append(Row(
                id: chat, title: g.name, key: g.gid,
                preview: last?.preview(model) ?? String(localized: "\(g.members.count) members"),
                date: last?.date, badge: String(localized: "Group · flood"), unread: model.unread(chat)
            ))
        }
        for c in model.contacts {
            let chat = ChatID.contact(key: c.key)
            let last = model.last(chat)
            out.append(Row(
                id: chat, title: c.name, key: c.key, preview: last?.preview(model) ?? "",
                date: last?.date, badge: hopsBadge(c.hops), unread: model.unread(chat)
            ))
        }
        return out.sorted { ($0.date ?? .distantPast, $1.title) > ($1.date ?? .distantPast, $0.title) }
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
                HStack(alignment: .firstTextBaseline) {
                    Text(row.title).font(.system(size: 16, weight: .semibold)).lineLimit(1)
                    Spacer()
                    if let d = row.date {
                        Text(d, style: .time).font(.system(size: 13)).foregroundStyle(Theme.tx2)
                    }
                }
                if !row.preview.isEmpty {
                    Text(row.preview).font(.system(size: 14)).foregroundStyle(Theme.tx2).lineLimit(1)
                }
                HStack {
                    TransportBadge(kind: .lora, label: row.badge)
                    Spacer()
                    if row.unread > 0 {
                        Text(String(row.unread))
                            .font(.system(size: 12, weight: .bold))
                            .padding(.horizontal, 6)
                            .frame(minWidth: 20, minHeight: 20)
                            .background(Theme.actBg, in: Capsule())
                            .foregroundStyle(Theme.actFg)
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
                Text(headline).font(.system(size: 15, weight: .semibold)).lineLimit(1)
                Spacer()
                if let r = model.radio, model.isConnected {
                    Text("BLE · SF\(r.sf) / \(bandwidth(r.bwHz))")
                        .font(.system(size: 13))
                        .foregroundStyle(Theme.tx2)
                }
            }
            AirtimeMeter(usedMs: model.budget.usedMs, limitMs: model.budget.limitMs)
            HStack(spacing: 6) {
                TransportBadge(kind: .lora, label: "LoRa")
                Text(model.isConnected ? "available" : "unavailable")
                    .font(.system(size: 12))
                    .foregroundStyle(Theme.tx2)
                Spacer()
                if model.bridgeOnline { TransportBadge(kind: .iroh, label: "iroh") }
                if model.torStatus == .ready { TransportBadge(kind: .tor, label: "Tor") }
                if !model.bridgeOnline && model.torStatus != .ready {
                    Text("No Internet bridge")
                        .font(.system(size: 11))
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
            Text("No conversations yet").font(.system(size: 17, weight: .semibold))
            Text("Add a contact by scanning their code, in person.")
                .font(.system(size: 14))
                .foregroundStyle(Theme.tx2)
                .multilineTextAlignment(.center)
            Button("Add a contact", action: addContact)
                .font(.system(size: 15, weight: .semibold))
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

