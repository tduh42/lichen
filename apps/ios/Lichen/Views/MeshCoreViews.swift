import SwiftUI

// MeshCore chats (protocol.md §15, ADR-0026): plain text with people who use a stock MeshCore
// app, kept apart from lichen's own chats and said to be weaker.

extension ChannelKind {
    /// "MeshCore channel · public".
    var label: String {
        switch self {
        case .publicChannel: String(localized: "MeshCore channel · public")
        case .hashtagChannel: String(localized: "MeshCore channel · hashtag")
        case .privateChannel: String(localized: "MeshCore channel · private")
        }
    }
}

/// What a MeshCore chat is, and for a channel: its kind, its secret to share, leaving it.
struct PlainInfoSheet: View {
    let chat: ChatID
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    @State private var confirmLeave = false

    private var channel: MeshChannel? {
        guard case let .channel(id) = chat else { return nil }
        return model.channels.first { $0.id == id }
    }

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 14) {
                    Banner(kind: .warn, title: String(localized: "Not lichen-encrypted"),
                           message: channel == nil ? directText : channelText)
                    if let c = channel {
                        CardList {
                            CardRow(title: c.name, subtitle: c.kind.label)
                        }
                        if c.kind == .hashtagChannel {
                            Text("Anyone who joins \(c.name) in a MeshCore app reads it and can write in it.")
                                .font(.system(.footnote)).foregroundStyle(Theme.tx2)
                        }
                        Button(role: .destructive) { confirmLeave = true } label: {
                            Text("Leave the channel").frame(maxWidth: .infinity, minHeight: 44)
                        }
                        .buttonStyle(.bordered)
                    }
                    Text("lichen chats with the same people stay end-to-end encrypted; nothing from them ever goes into a MeshCore chat.")
                        .font(.system(.footnote)).foregroundStyle(Theme.tx2)
                }
                .padding(16)
            }
            .background(Theme.bg)
            .navigationTitle(channel == nil ? String(localized: "MeshCore chat") : String(localized: "Channel"))
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .confirmationAction) { Button("Done") { dismiss() } }
            }
            .confirmationDialog("Leave \(channel?.name ?? "")?", isPresented: $confirmLeave, titleVisibility: .visible) {
                Button("Leave", role: .destructive) {
                    if let c = channel { model.leaveChannel(c.id) }
                    dismiss()
                }
            } message: {
                Text("The radio stops following it. Its messages stay here until you delete the conversation.")
            }
        }
        .presentationDetents([.medium, .large])
    }

    private var directText: String {
        String(localized: "These messages use MeshCore's own encryption, between your two radios: AES-128 with a short check, no forward secrecy, and the radios see the text. Good for talking with people who use a MeshCore app; use lichen for anything private.")
    }

    private var channelText: String {
        switch channel?.kind {
        case .publicChannel?:
            String(localized: "Everyone in range with a MeshCore radio reads this channel. Posts are flooded once and never acknowledged.")
        default:
            String(localized: "Everyone who has this channel's secret reads it. Posts are flooded once and never acknowledged.")
        }
    }
}

/// Joins a MeshCore channel: Public, a hashtag, or a private one from its secret (§15.3).
struct JoinChannelSheet: View {
    let open: (ChatID) -> Void
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    @State private var tag = ""
    @State private var name = ""
    @State private var secret = ""
    @State private var error: String?

    private var hasPublic: Bool { model.channels.contains { $0.kind == .publicChannel } }

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    Button { join(.publicChannel) } label: {
                        Label(hasPublic ? String(localized: "Public: already joined") : String(localized: "Join Public"),
                              systemImage: "globe")
                    }
                    .disabled(hasPublic)
                } footer: {
                    Text("MeshCore's public channel: everyone in range reads it. It starts muted.")
                }
                Section {
                    TextField("#hiking", text: $tag)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                    Button("Join the hashtag") { join(.hashtagChannel) }
                        .disabled(tag.trimmingCharacters(in: .whitespaces).isEmpty)
                } header: {
                    Text("Hashtag channel")
                } footer: {
                    Text("Anyone who knows the tag reads it, as in the MeshCore apps.")
                }
                Section {
                    TextField("Name", text: $name)
                    TextField("Secret: 32 hexadecimal digits", text: $secret)
                        .font(.system(.body, design: .monospaced))
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                    Button("Join the private channel") { join(.privateChannel) }
                        .disabled(name.isEmpty || secret.isEmpty)
                } header: {
                    Text("Private channel")
                } footer: {
                    Text("Its secret comes from whoever created it.")
                }
                if let error {
                    Text(error).foregroundStyle(Theme.danger)
                }
            }
            .scrollContentBackground(.hidden)
            .background(Theme.bg)
            .navigationTitle("Join a channel")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } }
            }
        }
    }

    private func join(_ kind: ChannelKind) {
        let label = kind == .hashtagChannel ? tag : name
        switch model.joinChannel(kind, name: label, secret: secret) {
        case let .success(chat):
            dismiss()
            open(chat)
        case let .failure(e):
            error = e.message
        }
    }
}
