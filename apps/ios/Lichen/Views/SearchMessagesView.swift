import SwiftUI

/// Search in the messages on this phone: conversations by name, then text messages, newest
/// first, case and accents aside. Nothing goes on the air.
struct SearchMessagesView: View {
    /// Opens a conversation, on a message when one was picked.
    let open: (ChatID, Int64?) -> Void
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    @State private var query = ""
    @FocusState private var focused: Bool

    var body: some View {
        NavigationStack {
            List {
                Section {
                    HStack(spacing: 8) {
                        Image(systemName: "magnifyingglass").foregroundStyle(Theme.tx2)
                        TextField("Search messages", text: $query)
                            .focused($focused)
                            .submitLabel(.search)
                            .autocorrectionDisabled()
                        if !query.isEmpty {
                            Button { query = "" } label: { Image(systemName: "xmark.circle.fill") }
                                .foregroundStyle(Theme.tx2)
                                .accessibilityLabel("Clear")
                        }
                    }
                    .frame(minHeight: 44)
                }
                .listRowBackground(Theme.s1)
                if !chats.isEmpty {
                    Section("Conversations") {
                        ForEach(chats, id: \.self) { chat in
                            Button { open(chat, nil) } label: {
                                Text(model.title(of: chat)).font(.system(.callout, weight: .semibold))
                                    .frame(maxWidth: .infinity, minHeight: 44, alignment: .leading)
                                    .contentShape(Rectangle())
                            }
                            .buttonStyle(.plain)
                        }
                    }
                    .listRowBackground(Theme.s1)
                }
                if !results.isEmpty {
                    Section("Messages") {
                        ForEach(results, id: \.id) { m in
                            Button { open(m.chat, m.id) } label: { ResultRow(message: m, query: trimmed) }
                                .buttonStyle(.plain)
                        }
                    }
                    .listRowBackground(Theme.s1)
                } else if trimmed.count >= 2 && chats.isEmpty {
                    Text("No messages found")
                        .font(.system(.subheadline))
                        .foregroundStyle(Theme.tx2)
                        .listRowBackground(Color.clear)
                }
            }
            .scrollContentBackground(.hidden)
            .background(Theme.bg)
            .navigationTitle("Search messages")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Close") { dismiss() }
                }
            }
            .onAppear {
                focused = true
                #if DEBUG
                if let q = UserDefaults.standard.string(forKey: "query") { query = q }
                #endif
            }
        }
    }

    private var trimmed: String { query.trimmingCharacters(in: .whitespaces) }

    private var results: [StoredMessage] { model.searchMessages(trimmed) }

    /// Conversations whose name contains the query.
    private var chats: [ChatID] {
        guard trimmed.count >= 2 else { return [] }
        let all = model.groups.map { ChatID.group(gid: $0.gid) } + model.contacts.map { ChatID.contact(key: $0.key) }
        return all.filter {
            model.title(of: $0).range(of: trimmed, options: [.caseInsensitive, .diacriticInsensitive]) != nil
        }
    }
}

/// A found message: its conversation, when, and the text around the match in bold.
private struct ResultRow: View {
    let message: StoredMessage
    let query: String
    @Environment(AppModel.self) private var model

    var body: some View {
        VStack(alignment: .leading, spacing: 3) {
            HStack {
                Text(model.title(of: message.chat)).font(.system(.callout, weight: .semibold)).lineLimit(1)
                Spacer()
                Text(message.date.formatted(date: .abbreviated, time: .shortened))
                    .font(.system(.footnote))
                    .foregroundStyle(Theme.tx2)
            }
            Text(snippet)
                .font(.system(.subheadline))
                .foregroundStyle(Theme.tx2)
                .lineLimit(2)
        }
        .padding(.vertical, 4)
        .contentShape(Rectangle())
        .accessibilityElement(children: .combine)
    }

    /// "Camille: …the text around the match…", the match in bold.
    private var snippet: AttributedString {
        guard case let .text(_, text) = message.message else { return AttributedString(message.message.display) }
        let who: String = {
            if message.outgoing { return String(localized: "You") + ": " }
            if let a = message.author { return model.name(of: a) + ": " }
            return ""
        }()
        // Some context before the match, so that it shows in two lines.
        var shown = Substring(text)
        if let r = text.range(of: query, options: [.caseInsensitive, .diacriticInsensitive]),
           text.distance(from: text.startIndex, to: r.lowerBound) > 40 {
            shown = Substring("…" + text[text.index(r.lowerBound, offsetBy: -30)...])
        }
        var out = AttributedString(who + shown)
        let plain = String(out.characters)
        if let r = plain.range(of: query, options: [.caseInsensitive, .diacriticInsensitive]),
           let lower = AttributedString.Index(r.lowerBound, within: out),
           let upper = AttributedString.Index(r.upperBound, within: out) {
            out[lower..<upper].font = .system(.subheadline, weight: .bold)
            out[lower..<upper].foregroundColor = Theme.tx
        }
        return out
    }
}
