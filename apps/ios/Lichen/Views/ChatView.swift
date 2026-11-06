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
                        ForEach(messages) { m in
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
        .onAppear { model.markRead(chat) }
        .alert("Not sent", isPresented: Binding(get: { error != nil }, set: { if !$0 { error = nil } })) {
            Button("OK", role: .cancel) {}
        } message: {
            Text(error ?? "")
        }
    }

    private var messages: [ChatMessage] { model.chats[chat] ?? [] }

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
    let message: ChatMessage
    let isGroup: Bool
    @Environment(AppModel.self) private var model

    var body: some View {
        VStack(alignment: message.outgoing ? .trailing : .leading, spacing: 3) {
            if isGroup, !message.outgoing, let a = message.author {
                Text(model.name(of: a)).font(.caption.weight(.semibold)).foregroundStyle(Theme.secondaryText)
            }
            Text(message.text)
                .padding(.horizontal, 14)
                .padding(.vertical, 9)
                .background(message.outgoing ? Theme.accent : Theme.incomingBubble, in: RoundedRectangle(cornerRadius: 18))
                .foregroundStyle(message.outgoing ? .white : .primary)
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
            return String(localized: "Sending \(String(min(sent, total))) / \(String(total)) fragments")
        case .delivered:
            return String(localized: "Delivered · \(message.fragments) fragments · \(airtime) s of airtime")
        case let .failed(reason):
            return String(localized: "Not delivered: \(reason)")
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

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            if let line = costLine {
                Text(line).font(.caption2).foregroundStyle(Theme.secondaryText).padding(.leading, 6)
            }
            HStack(spacing: 8) {
                TextField("Message", text: $draft, axis: .vertical)
                    .lineLimit(1...5)
                    .padding(.horizontal, 14)
                    .padding(.vertical, 9)
                    .background(Color(.systemBackground), in: RoundedRectangle(cornerRadius: 19))
                    .overlay(RoundedRectangle(cornerRadius: 19).stroke(Color(.separator)))
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
        .padding(.horizontal, 10)
        .padding(.top, 8)
        .padding(.bottom, 10)
        .background(.bar)
    }

    private var costLine: String? {
        if case .group = chat { return String(localized: "Flooded to every member, without acknowledgement") }
        guard let e = model.estimate(chat, text: draft) else { return nil }
        let s = String(format: "%.1f", Double(e.airtimeMs) / 1000)
        return String(localized: "Estimated cost: \(Int(e.fragments)) fragments · ≈ \(s) s of airtime")
    }
}
