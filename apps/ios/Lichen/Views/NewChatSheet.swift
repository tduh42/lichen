import SwiftUI

/// Pick a contact, or create a group.
struct NewChatSheet: View {
    let open: (ChatID) -> Void
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    @State private var groupName = ""
    @State private var selected: Set<Data> = []
    @State private var error: String?
    @State private var search = ""
    @State private var joining = false

    /// People the node knows, lichen users first (their identity is known), then by name.
    private var people: [ContactInfo] {
        let all = model.contacts.filter {
            search.isEmpty || $0.name.localizedCaseInsensitiveContains(search)
        }
        return all.sorted {
            ($0.identity == nil ? 1 : 0, $0.name.lowercased()) < ($1.identity == nil ? 1 : 0, $1.name.lowercased())
        }
    }

    var body: some View {
        NavigationStack {
            List {
                Section {
                    if model.contacts.isEmpty {
                        Text("No contacts yet: they come from your node.").foregroundStyle(Theme.secondaryText)
                    }
                    // Someone without lichen gets a MeshCore chat (§15); a lichen user, a lichen one.
                    ForEach(people, id: \.key) { c in
                        Button { open(c.identity == nil ? .mesh(key: c.key) : .contact(key: c.key)) } label: {
                            HStack(spacing: 12) {
                                Avatar(name: c.name, key: c.key, size: 32)
                                VStack(alignment: .leading, spacing: 2) {
                                    Text(c.name).foregroundStyle(Theme.tx).lineLimit(1)
                                    Text(c.identity == nil ? String(localized: "MeshCore chat · not lichen-encrypted")
                                                           : String(localized: "lichen"))
                                        .font(.system(.caption))
                                        .foregroundStyle(Theme.tx2)
                                }
                                Spacer()
                                TransportBadge(kind: .lora, label: hopsBadge(c.hops))
                            }
                        }
                    }
                } header: {
                    Text("Contacts")
                } footer: {
                    Text("Radios your node has heard. With someone who has lichen, the chat is end-to-end encrypted; with someone who only has a MeshCore app, it is a MeshCore chat, readable by the radios.")
                }
                Section {
                    ForEach(model.channels, id: \.id) { c in
                        Button { open(.channel(id: c.id)) } label: {
                            HStack(spacing: 12) {
                                Image(systemName: c.kind == .publicChannel ? "globe" : "number")
                                    .frame(width: 32, height: 32)
                                    .background(Theme.s2, in: Circle())
                                    .foregroundStyle(Theme.tx2)
                                VStack(alignment: .leading, spacing: 2) {
                                    Text(c.name).foregroundStyle(Theme.tx)
                                    Text(c.kind.label).font(.system(.caption)).foregroundStyle(Theme.tx2)
                                }
                            }
                        }
                    }
                    Button { joining = true } label: { Label("Join a channel", systemImage: "plus") }
                } header: {
                    Text("MeshCore channels")
                } footer: {
                    Text("Channels your radio follows, as in the MeshCore apps. Not lichen-encrypted.")
                }
                Section {
                    TextField("Group name", text: $groupName)
                    ForEach(people.filter { $0.identity != nil }, id: \.key) { c in
                        Toggle(c.name, isOn: Binding(
                            get: { selected.contains(c.key) },
                            set: { if $0 { selected.insert(c.key) } else { selected.remove(c.key) } }
                        ))
                    }
                    Button("Create group") {
                        if let e = model.createGroup(name: groupName, members: Array(selected)) {
                            error = e
                        } else if let g = model.groups.first(where: { $0.name == groupName }) {
                            open(.group(gid: g.gid))
                        }
                    }
                    .disabled(groupName.isEmpty)
                } header: {
                    Text("New group")
                } footer: {
                    Text("Members are contacts you have already talked with in lichen; people next to you can also join by scanning the group's QR code. Every member gets the others' sender keys over encrypted sessions. Group messages are flooded and signed.")
                }
                if let error {
                    Text(error).foregroundStyle(Theme.danger)
                }
            }
            .scrollContentBackground(.hidden)
            .background(Theme.bg)
            .listRowBackground(Theme.s1)
            .searchable(text: $search, placement: .navigationBarDrawer(displayMode: .always))
            .sheet(isPresented: $joining) { JoinChannelSheet(open: open) }
            .navigationTitle("New conversation")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } }
            }
        }
    }
}
