import SwiftUI

/// Pick a contact, or create a group.
struct NewChatSheet: View {
    let open: (ChatID) -> Void
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    @State private var groupName = ""
    @State private var selected: Set<Data> = []
    @State private var error: String?

    var body: some View {
        NavigationStack {
            List {
                Section("Contacts") {
                    if model.contacts.isEmpty {
                        Text("No contacts yet: they come from your node.").foregroundStyle(Theme.secondaryText)
                    }
                    ForEach(model.contacts, id: \.key) { c in
                        Button { open(.contact(key: c.key)) } label: {
                            HStack(spacing: 12) {
                                Avatar(name: c.name, key: c.key, size: 32)
                                Text(c.name).foregroundStyle(Theme.tx)
                                Spacer()
                                TransportBadge(kind: .lora, label: hopsBadge(c.hops))
                            }
                        }
                    }
                }
                Section {
                    TextField("Group name", text: $groupName)
                    ForEach(model.contacts, id: \.key) { c in
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
                    .disabled(groupName.isEmpty || selected.isEmpty)
                } header: {
                    Text("New group")
                } footer: {
                    Text("Every member gets the others' sender keys over encrypted sessions. Group messages are flooded and signed.")
                }
                if let error {
                    Text(error).foregroundStyle(Theme.danger)
                }
            }
            .scrollContentBackground(.hidden)
            .background(Theme.bg)
            .listRowBackground(Theme.s1)
            .navigationTitle("New conversation")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } }
            }
        }
    }
}
