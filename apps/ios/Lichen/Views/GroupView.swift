import SwiftUI

/// A group: members and their keys, location, diffusion (mockup `groupe`).
struct GroupView: View {
    let gid: Data
    @Environment(AppModel.self) private var model
    @State private var sharing = false
    @State private var error: String?

    var body: some View {
        ScrollView {
            if let g = group {
                VStack(alignment: .leading, spacing: 8) {
                    keysBanner(g)
                    SectionLabel("Members").padding(.top, 6)
                    CardList {
                        ForEach(Array(g.members.enumerated()), id: \.element.key) { i, m in
                            if i > 0 { CardDivider(inset: 68) }
                            memberRow(m, group: g)
                        }
                    }
                    SectionLabel("Location").padding(.top, 10)
                    CardList {
                        CardRow(title: String(localized: "Let this group locate me"),
                                subtitle: String(localized: "In search mode (L8)")) {
                            Toggle("Let this group locate me", isOn: .constant(false)).labelsHidden().disabled(true)
                        }
                        CardDivider()
                        Button { sharing = true } label: {
                            CardRow(title: String(localized: "Precision shared with the group")) {
                                Text(model.share(of: .group(gid: gid)).map { PrecisionText.name($0.precision) }
                                     ?? String(localized: "Not shared"))
                                    .font(.system(size: 15))
                                    .foregroundStyle(Theme.tx2)
                            }
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                    }
                    SectionLabel("Diffusion").padding(.top, 10)
                    CardList {
                        CardRow(title: String(localized: "Hop limit"),
                                subtitle: String(localized: "Each hop costs the relays airtime; messages fit up to 8")) {
                            Text("8").font(Theme.figure(17)).foregroundStyle(Theme.tx2)
                        }
                        CardDivider()
                        CardRow(title: String(localized: "Media"),
                                subtitle: String(localized: "Never flooded: a LoRa announcement, the content over iroh or Tor (L7)"))
                    }
                    HStack(spacing: 8) {
                        Button("Invite") {}.buttonStyle(.secondary)
                        Button("Leave the group") {}.buttonStyle(.destructive)
                    }
                    .disabled(true)
                    .padding(.top, 12)
                    LaterNote(text: "Inviting and leaving come with the phone identity.")
                }
                .padding(16)
            }
        }
        .screen()
        .navigationBarTitleDisplayMode(.inline)
        .toolbarBackground(Theme.s1, for: .navigationBar)
        .toolbarBackground(.visible, for: .navigationBar)
        .toolbar {
            ToolbarItem(placement: .principal) {
                VStack(spacing: 2) {
                    Text(group?.name ?? "").font(.system(size: 17, weight: .semibold))
                    Text("\(group?.members.count ?? 0) members · flood")
                        .font(.system(size: 12)).foregroundStyle(Theme.tx2)
                }
            }
        }
        .sheet(isPresented: $sharing) { ShareSheet() }
        .alert("Not changed", isPresented: Binding(get: { error != nil }, set: { if !$0 { error = nil } })) {
            Button("OK", role: .cancel) {}
        } message: {
            Text(error ?? "")
        }
    }

    private var group: GroupInfo? { model.groups.first { $0.gid == gid } }

    private var isAdmin: Bool { group?.admin == model.nodeKey }

    @ViewBuilder private func keysBanner(_ g: GroupInfo) -> some View {
        let others = max(g.members.count - 1, 0)
        if Int(g.keysKnown) >= others {
            Banner(kind: .ok, title: String(localized: "Sender keys up to date"),
                   message: String(localized: "\(Int(g.keysKnown)) / \(others) members' keys received; a departure rotates them."))
        } else {
            Banner(kind: .warn, title: String(localized: "Waiting for sender keys"),
                   message: String(localized: "\(Int(g.keysKnown)) / \(others) received. Messages from the others show once their key arrives."))
        }
    }

    private func memberRow(_ m: ContactInfo, group g: GroupInfo) -> some View {
        let me = m.key == model.nodeKey
        let admin = m.key == g.admin
        return HStack(spacing: 12) {
            Avatar(name: me ? String(localized: "You") : m.name, key: m.key, size: 40)
            VStack(alignment: .leading, spacing: 2) {
                Text(me ? String(localized: "You") : m.name).font(.system(size: 16))
                Text(admin ? String(localized: "Admin") : String(localized: "Member"))
                    .font(.system(size: 13)).foregroundStyle(Theme.tx2)
            }
            Spacer()
            if !me {
                // Keys are compared in person with the phone identity.
                Badge(text: String(localized: "to verify"), background: Theme.warnBg, foreground: Theme.warnFg)
            }
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 10)
        .contextMenu {
            if isAdmin && !me {
                Button(role: .destructive) {
                    error = model.removeMember(m.key, from: gid)
                } label: {
                    Label("Remove from the group", systemImage: "person.badge.minus")
                }
            }
        }
    }
}
