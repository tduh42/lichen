import SwiftUI

/// Where to forward a message (ADR-0027): it goes as a new message of ours. A MeshCore chat
/// takes text only; files and voice messages go to lichen contacts.
struct ForwardSheet: View {
    let message: StoredMessage
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    @State private var error: String?

    private enum Fits { case text, any, contactsOnly }

    private var fits: Fits {
        switch message.message {
        case .text: .text
        case .sticker, .gif: .any
        default: .contactsOnly
        }
    }

    var body: some View {
        NavigationStack {
            List {
                if fits != .contactsOnly, !model.groups.isEmpty {
                    Section("Groups") {
                        ForEach(model.groups, id: \.gid) { g in
                            row(g.name, key: g.gid, to: .group(gid: g.gid))
                        }
                    }
                }
                Section("Contacts") {
                    ForEach(model.contacts.filter { $0.identity != nil }, id: \.key) { c in
                        row(c.name, key: c.key, to: .contact(key: c.key))
                    }
                }
                if fits == .text {
                    Section {
                        ForEach(model.channels, id: \.id) { c in
                            row(c.name, key: c.id, to: .channel(id: c.id))
                        }
                        ForEach(model.contacts.filter { $0.identity == nil }, id: \.key) { c in
                            row(c.name, key: c.key, to: .mesh(key: c.key))
                        }
                    } header: {
                        Text("MeshCore")
                    } footer: {
                        Text("Not lichen-encrypted.")
                    }
                }
                if let error {
                    Text(error).foregroundStyle(Theme.danger)
                }
            }
            .scrollContentBackground(.hidden)
            .background(Theme.bg)
            .navigationTitle("Forward")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } }
            }
        }
    }

    private func row(_ name: String, key: Data, to chat: ChatID) -> some View {
        Button {
            if let e = model.forward(message, to: chat) { error = e } else { dismiss() }
        } label: {
            HStack(spacing: 12) {
                Avatar(name: name, key: key, size: 32)
                Text(name).foregroundStyle(Theme.tx)
            }
        }
    }
}

/// Where to send a file or a photo another app opened in lichen (ADR-0034): a lichen contact,
/// or a group (over the Internet bridge). Its conversation then opens on the usual photo or
/// file sheet, which says what it costs and how it goes; blocked contacts are left out.
struct ShareTargetSheet: View {
    let shared: AppModel.SharedFile
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            List {
                Section {
                    HStack(spacing: 12) {
                        Image(systemName: shared.isImage ? "photo" : "doc")
                            .frame(width: 32, height: 32)
                            .background(Theme.s2, in: RoundedRectangle(cornerRadius: 8))
                        VStack(alignment: .leading, spacing: 2) {
                            Text(verbatim: shared.name).font(.system(.subheadline, weight: .semibold)).lineLimit(2)
                            Text(ByteCountFormatter.string(fromByteCount: Int64(shared.data.count), countStyle: .file))
                                .font(.system(.caption))
                                .foregroundStyle(Theme.tx2)
                        }
                    }
                }
                if !model.groups.isEmpty {
                    Section("Groups") {
                        ForEach(model.groups, id: \.gid) { g in
                            row(g.name, key: g.gid, to: .group(gid: g.gid))
                        }
                    }
                }
                Section("Contacts") {
                    ForEach(model.contacts.filter { $0.identity != nil && !model.isBlocked($0.key) }, id: \.key) { c in
                        row(c.name, key: c.key, to: .contact(key: c.key))
                    }
                }
            }
            .scrollContentBackground(.hidden)
            .background(Theme.bg)
            .navigationTitle("Send to")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } }
            }
        }
    }

    private func row(_ name: String, key: Data, to chat: ChatID) -> some View {
        Button {
            model.share(shared, to: chat)
            dismiss()
        } label: {
            HStack(spacing: 12) {
                Avatar(name: name, key: key, size: 32)
                Text(name).foregroundStyle(Theme.tx)
            }
        }
    }
}
