import SwiftUI

/// New conversation (F-4): search, three actions (New group, Join a channel, Add a contact),
/// the radio's channels, the groups, then the people, last: the radio may know hundreds of
/// MeshCore radios, and nothing else should sit below them.
struct NewChatSheet: View {
    let open: (ChatID) -> Void
    /// Add a contact: the sheet closes, then Messages opens Add a contact.
    var addContact: () -> Void = {}
    /// New group: the sheet closes, then Messages shows the New group screen.
    var newGroup: () -> Void = {}
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    @Environment(\.dynamicTypeSize) private var typeSize
    @State private var search = ""
    @State private var joining = false
    @State private var filter = PeopleFilter.all
    @State private var error: String?

    enum PeopleFilter: Hashable { case all, lichen, meshCore }

    private var query: String { search.trimmingCharacters(in: .whitespacesAndNewlines) }

    private func matches(_ name: String) -> Bool {
        query.isEmpty || name.localizedCaseInsensitiveContains(query)
    }

    /// People the search finds, lichen users first (their identity is known), then by name.
    private var people: [ContactInfo] {
        model.contacts.filter { matches($0.name) }.sorted {
            ($0.identity == nil ? 1 : 0, $0.name.lowercased()) < ($1.identity == nil ? 1 : 0, $1.name.lowercased())
        }
    }

    private var shownPeople: [ContactInfo] {
        switch filter {
        case .all: people
        case .lichen: people.filter { $0.identity != nil }
        case .meshCore: people.filter { $0.identity == nil }
        }
    }

    /// The radio's channels the search finds, Public first; not the `#tag` typed, shown once
    /// on top.
    private var channels: [MeshChannel] {
        model.channels.filter { matches($0.name) && $0.id != followed?.id }.sorted {
            ($0.kind == .publicChannel ? 0 : 1, $0.name.lowercased()) < ($1.kind == .publicChannel ? 0 : 1, $1.name.lowercased())
        }
    }

    private var groups: [GroupInfo] { model.groups.filter { matches($0.name) } }

    /// `#tag` typed: the hashtag channel's name, as the radio keeps it (`#tag`).
    private var hashtag: String? {
        guard query.hasPrefix("#") else { return nil }
        return hashtagChannelName(tag: query)
    }

    /// The channel of the `#tag` typed, when the radio follows it already.
    private var followed: MeshChannel? {
        hashtag.flatMap { tag in model.channels.first { $0.name.lowercased() == tag } }
    }

    var body: some View {
        NavigationStack {
            List {
                if let tag = hashtag {
                    Section {
                        if let followed {
                            channelRow(followed)
                        } else {
                            Button { joinHashtag(tag) } label: {
                                Label(String(localized: "Join \(tag)"), systemImage: "number")
                                    .foregroundStyle(Theme.tx)
                            }
                        }
                    }
                }
                Section {
                    actions
                        .listRowInsets(EdgeInsets(top: 4, leading: 0, bottom: 4, trailing: 0))
                        .listRowBackground(Color.clear)
                    if let error {
                        Text(error).foregroundStyle(Theme.danger)
                    }
                }
                if !channels.isEmpty {
                    Section {
                        ForEach(channels, id: \.id) { channelRow($0) }
                    } header: {
                        Text("MeshCore channels")
                    } footer: {
                        Text("Channels your radio follows, as in the MeshCore apps. Not lichen-encrypted.")
                    }
                }
                if !groups.isEmpty {
                    Section {
                        ForEach(groups, id: \.gid) { g in
                            Button { open(.group(gid: g.gid)) } label: {
                                HStack(spacing: 12) {
                                    Avatar(name: g.name, key: g.gid, size: 32)
                                    VStack(alignment: .leading, spacing: 2) {
                                        Text(g.name).foregroundStyle(Theme.tx).lineLimit(1)
                                        Text("\(g.members.count) members")
                                            .font(.system(.caption))
                                            .foregroundStyle(Theme.tx2)
                                    }
                                }
                            }
                        }
                    } header: {
                        Text("Groups")
                    }
                }
                peopleSection
            }
            .scrollContentBackground(.hidden)
            .background(Theme.bg)
            .listRowBackground(Theme.s1)
            .searchable(text: $search, placement: .navigationBarDrawer(displayMode: .always),
                        prompt: Text("Search people, channels and groups"))
            .autocorrectionDisabled()
            .textInputAutocapitalization(.never)
            .sheet(isPresented: $joining) { JoinChannelSheet(open: open) }
            .navigationTitle("New conversation")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } }
            }
        }
    }

    /// New group, Join a channel, Add a contact: side by side, stacked at the largest text sizes.
    @ViewBuilder private var actions: some View {
        let tiles = Group {
            ActionTile(title: String(localized: "New group"), symbol: "person.3") {
                newGroup()
                dismiss()
            }
            ActionTile(title: String(localized: "Join a channel"), symbol: "number") { joining = true }
            ActionTile(title: String(localized: "Add a contact"), symbol: "qrcode") {
                addContact()
                dismiss()
            }
        }
        if typeSize.isAccessibilitySize {
            VStack(spacing: 8) { tiles }
        } else {
            // One height for the three, the tallest label's.
            HStack(spacing: 8) { tiles }.fixedSize(horizontal: false, vertical: true)
        }
    }

    private func channelRow(_ c: MeshChannel) -> some View {
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

    /// The people, last: a filter with counts, then the list.
    @ViewBuilder private var peopleSection: some View {
        let lichen = people.filter { $0.identity != nil }.count
        Section {
            if !model.contacts.isEmpty {
                Picker("Contacts", selection: $filter) {
                    Text("All (\(people.count))").tag(PeopleFilter.all)
                    Text("lichen (\(lichen))").tag(PeopleFilter.lichen)
                    Text("MeshCore (\(people.count - lichen))").tag(PeopleFilter.meshCore)
                }
                .pickerStyle(.segmented)
                .listRowBackground(Color.clear)
                .listRowInsets(EdgeInsets(top: 4, leading: 0, bottom: 4, trailing: 0))
            }
            if model.contacts.isEmpty {
                Text("No contacts yet: they come from your node.").foregroundStyle(Theme.secondaryText)
            } else if shownPeople.isEmpty {
                Text("Nobody matches.").foregroundStyle(Theme.secondaryText)
            }
            // Someone without lichen gets a MeshCore chat (§15); a lichen user, a lichen one.
            ForEach(shownPeople, id: \.key) { c in
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
            Text("lichen chats are end-to-end encrypted; MeshCore chats can be read by the radios.")
        }
    }

    private func joinHashtag(_ tag: String) {
        switch model.joinChannel(.hashtagChannel, name: tag) {
        case let .success(chat): open(chat)
        case let .failure(e): error = e.message
        }
    }
}

/// One of New conversation's three actions: an icon over its name.
private struct ActionTile: View {
    let title: String
    let symbol: String
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            VStack(spacing: 6) {
                Image(systemName: symbol).font(.system(.title3))
                Text(title)
                    .font(.system(.footnote, weight: .medium))
                    .multilineTextAlignment(.center)
                    .lineLimit(2)
                    .fixedSize(horizontal: false, vertical: true)
            }
            .foregroundStyle(Theme.tx)
            .frame(maxWidth: .infinity, minHeight: 64, maxHeight: .infinity)
            .padding(.vertical, 8)
            .background(Theme.s1, in: RoundedRectangle(cornerRadius: 14))
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityLabel(title)
    }
}

/// New group (F-4): its name, the lichen contacts to add (searchable), Create.
struct NewGroupView: View {
    let open: (ChatID) -> Void
    @Environment(AppModel.self) private var model
    @State private var name = ""
    @State private var selected: Set<Data> = []
    @State private var search = ""
    @State private var error: String?

    /// Members are people we already talked with in lichen: their identity is known.
    private var candidates: [ContactInfo] {
        let q = search.trimmingCharacters(in: .whitespacesAndNewlines)
        return model.contacts
            .filter { $0.identity != nil && (q.isEmpty || $0.name.localizedCaseInsensitiveContains(q)) }
            .sorted { $0.name.lowercased() < $1.name.lowercased() }
    }

    var body: some View {
        let trimmed = name.trimmingCharacters(in: .whitespacesAndNewlines)
        List {
            Section {
                TextField("Group name", text: $name)
            }
            Section {
                if candidates.isEmpty {
                    Text("Nobody matches.").foregroundStyle(Theme.secondaryText)
                }
                ForEach(candidates, id: \.key) { c in
                    Button {
                        if selected.contains(c.key) { selected.remove(c.key) } else { selected.insert(c.key) }
                    } label: {
                        HStack(spacing: 12) {
                            Avatar(name: c.name, key: c.key, size: 32)
                            Text(c.name).foregroundStyle(Theme.tx).lineLimit(1)
                            Spacer()
                            Image(systemName: selected.contains(c.key) ? "checkmark.circle.fill" : "circle")
                                .foregroundStyle(selected.contains(c.key) ? Theme.tx : Theme.tx2)
                        }
                    }
                    .accessibilityAddTraits(selected.contains(c.key) ? .isSelected : [])
                }
            } header: {
                Text("Members")
            } footer: {
                Text("Members are contacts you have already talked with in lichen; people next to you can also join by scanning the group’s QR code. Every member gets the others’ sender keys over encrypted sessions. Group messages are flooded and signed.")
            }
            if let error {
                Text(error).foregroundStyle(Theme.danger)
            }
        }
        .scrollContentBackground(.hidden)
        .background(Theme.bg)
        .searchable(text: $search, placement: .navigationBarDrawer(displayMode: .always))
        .navigationTitle("New group")
        .navigationBarTitleDisplayMode(.inline)
        .safeAreaInset(edge: .bottom) {
            Button("Create group") {
                if let e = model.createGroup(name: trimmed, members: Array(selected)) {
                    error = e
                } else if let g = model.groups.first(where: { $0.name == trimmed }) {
                    open(.group(gid: g.gid))
                }
            }
            .buttonStyle(.primary)
            .disabled(trimmed.isEmpty)
            .padding(16)
            .background(Theme.bg)
        }
    }
}
