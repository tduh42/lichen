import SwiftUI

/// "Messages": node status, airtime budget, then every conversation (mockup ios-conversations).
struct ConversationsView: View {
    @Environment(AppModel.self) private var model
    @State private var filter = Filter.all
    @State private var query = ""
    @State private var composing = false
    @State private var path: [ChatID] = []

    enum Filter: Hashable { case all, groups, contacts }

    var body: some View {
        NavigationStack(path: $path) {
            ScrollView {
                VStack(spacing: 12) {
                    StatusCard()
                    Picker("Filter", selection: $filter) {
                        Text("All").tag(Filter.all)
                        Text("Groups").tag(Filter.groups)
                        Text("Contacts").tag(Filter.contacts)
                    }
                    .pickerStyle(.segmented)
                    if rows.isEmpty {
                        Text(model.isConnected ? "No conversations yet." : "Connect your MeshCore node in Settings.")
                            .font(.subheadline)
                            .foregroundStyle(Theme.secondaryText)
                            .padding(.top, 40)
                    } else {
                        LazyVStack(spacing: 0) {
                            ForEach(rows) { row in
                                NavigationLink(value: row.id) { ChatRow(row: row) }
                                    .buttonStyle(.plain)
                                Divider().padding(.leading, 72)
                            }
                        }
                        .background(Color(.secondarySystemGroupedBackground), in: RoundedRectangle(cornerRadius: 14))
                    }
                }
                .padding(.horizontal, 16)
                .padding(.bottom, 16)
            }
            .background(Color(.systemGroupedBackground))
            .navigationTitle("Messages")
            .searchable(text: $query, prompt: "Search conversations")
            .navigationDestination(for: ChatID.self) { ChatView(chat: $0) }
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button { composing = true } label: { Image(systemName: "square.and.pencil") }
                        .accessibilityLabel("New message")
                }
            }
            .onChange(of: model.debugOpen) {
                if let chat = model.debugOpen { path = [chat] }
            }
            .sheet(isPresented: $composing) {
                NewChatSheet { chat in
                    composing = false
                    path.append(chat)
                }
            }
        }
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
        if filter != .contacts {
            for g in model.groups {
                let chat = ChatID.group(gid: g.gid)
                let last = model.last(chat)
                out.append(Row(id: chat, title: g.name, key: g.gid, preview: last?.preview(model) ?? String(localized: "\(String(g.members.count)) members"), date: last?.date, badge: String(localized: "Group · flood"), unread: model.unread(chat)))
            }
        }
        if filter != .groups {
            for c in model.contacts {
                let chat = ChatID.contact(key: c.key)
                let last = model.last(chat)
                out.append(Row(id: chat, title: c.name, key: c.key, preview: last?.preview(model) ?? "", date: last?.date, badge: hopsBadge(c.hops), unread: model.unread(chat)))
            }
        }
        let q = query.trimmingCharacters(in: .whitespaces).lowercased()
        return out
            .filter { q.isEmpty || $0.title.lowercased().contains(q) || $0.preview.lowercased().contains(q) }
            .sorted { ($0.date ?? .distantPast, $1.title) > ($1.date ?? .distantPast, $0.title) }
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
        HStack(spacing: 12) {
            Avatar(name: row.title, key: row.key)
            VStack(alignment: .leading, spacing: 3) {
                HStack(alignment: .firstTextBaseline) {
                    Text(row.title).font(.body.weight(.semibold))
                    Spacer()
                    if let d = row.date {
                        Text(d, style: .time).font(.footnote).foregroundStyle(Theme.secondaryText)
                    }
                }
                if !row.preview.isEmpty {
                    Text(row.preview).font(.subheadline).foregroundStyle(Theme.secondaryText).lineLimit(1)
                }
                HStack {
                    Badge(text: row.badge, background: Theme.accentSoft, foreground: Theme.accentText)
                    Spacer()
                    if row.unread > 0 {
                        Text(String(row.unread))
                            .font(.caption.weight(.semibold))
                            .padding(.horizontal, 6)
                            .frame(minWidth: 20, minHeight: 20)
                            .background(Theme.accent, in: Capsule())
                            .foregroundStyle(.white)
                    }
                }
            }
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 12)
        .contentShape(Rectangle())
    }
}

/// Node, radio and airtime left this hour (ADR-0007).
struct StatusCard: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack {
                Circle().fill(model.isConnected ? Theme.ok : Color.gray).frame(width: 8, height: 8)
                Text(headline).font(.subheadline.weight(.semibold))
                Spacer()
                if let r = model.radio {
                    Text("BLE · SF\(String(r.sf)) / \(bandwidth(r.bwHz))")
                        .font(.footnote)
                        .foregroundStyle(Theme.secondaryText)
                }
            }
            VStack(spacing: 6) {
                HStack {
                    Text("Airtime left this hour").foregroundStyle(Theme.secondaryText)
                    Spacer()
                    Text("\(seconds(left)) s / \(seconds(model.budget.limitMs)) s").fontWeight(.semibold)
                }
                .font(.footnote)
                ProgressView(value: Double(left), total: Double(max(model.budget.limitMs, 1)))
                    .tint(Theme.accent)
            }
            HStack(spacing: 6) {
                Badge(text: "LoRa", background: Theme.accentSoft, foreground: Theme.accentText)
                Text(model.isConnected ? "available" : "unavailable")
                    .font(.caption)
                    .foregroundStyle(Theme.secondaryText)
            }
        }
        .padding(14)
        .background(Color(.secondarySystemGroupedBackground), in: RoundedRectangle(cornerRadius: 14))
    }

    private var left: UInt64 { model.budget.limitMs - min(model.budget.usedMs, model.budget.limitMs) }

    private var headline: String {
        switch model.companionState {
        case let .connected(name): return String(localized: "\(name) connected")
        case let .connecting(name): return String(localized: "Connecting to \(name)…")
        case .scanning: return String(localized: "Looking for nodes…")
        case .off: return String(localized: "Bluetooth is off")
        case .idle: return String(localized: "No node connected")
        }
    }
}

func seconds(_ ms: UInt64) -> String { String(ms / 1000) }

func bandwidth(_ hz: UInt32) -> String {
    hz % 1000 == 0 ? "\(hz / 1000) kHz" : String(format: "%.1f kHz", Double(hz) / 1000)
}
