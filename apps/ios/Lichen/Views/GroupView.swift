import SwiftUI

/// A group: members and their keys, location, diffusion (mockup `groupe`).
struct GroupView: View {
    let gid: Data
    @Environment(AppModel.self) private var model
    @State private var sharing = false
    @State private var error: String?
    @State private var consent = false
    @State private var inviting = false
    @State private var showingCode = false
    @State private var leaving = false
    @State private var removing: ContactInfo?
    @Environment(\.dismiss) private var dismiss

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
                    if isAdmin {
                        SectionLabel("Invite").padding(.top, 10)
                        CardList {
                            Button { showingCode = true } label: {
                                CardRow(title: String(localized: "Invite with a QR code"),
                                        subtitle: String(localized: "People next to you scan it and join, for 10 minutes")) {
                                    Image(systemName: "qrcode").foregroundStyle(Theme.tx2)
                                }
                                .contentShape(Rectangle())
                            }
                            .buttonStyle(.plain)
                            CardDivider()
                            Button { inviting = true } label: {
                                CardRow(title: String(localized: "Invite contacts")) {
                                    Image(systemName: "person.badge.plus").foregroundStyle(Theme.tx2)
                                }
                                .contentShape(Rectangle())
                            }
                            .buttonStyle(.plain)
                        }
                    }
                    SectionLabel("Location").padding(.top, 10)
                    CardList {
                        CardRow(title: String(localized: "Let this group locate me"),
                                subtitle: String(localized: "Your phone answers their search pings even outside SOS")) {
                            Toggle("Let this group locate me", isOn: Binding(get: { consent }, set: {
                                consent = $0
                                model.setSearchConsent(gid, $0)
                            }))
                            .labelsHidden()
                        }
                        CardDivider()
                        Button { sharing = true } label: {
                            CardRow(title: String(localized: "Precision shared with the group")) {
                                Text(model.share(of: .group(gid: gid)).map { PrecisionText.name($0.precision) }
                                     ?? String(localized: "Not shared"))
                                    .font(.system(.subheadline))
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
                                subtitle: String(localized: "Never flooded: a LoRa announcement, the content over iroh or Tor"))
                    }
                    Button(isAdmin ? "Close the group" : "Leave the group") { leaving = true }
                        .buttonStyle(.destructive)
                        .padding(.top, 12)
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
                    Text(group?.name ?? "").font(.system(.body, weight: .semibold))
                    Text("\(group?.members.count ?? 0) members · flood")
                        .font(.system(.caption)).foregroundStyle(Theme.tx2)
                }
            }
        }
        .sheet(isPresented: $sharing) { ChatShareSheet(chat: .group(gid: gid)) }
        .sheet(isPresented: $inviting) { InviteSheet(gid: gid) }
        .sheet(isPresented: $showingCode) { InviteCodeSheet(gid: gid) }
        .confirmationDialog(isAdmin ? "Close the group?" : "Leave the group?", isPresented: $leaving, titleVisibility: .visible) {
            Button(isAdmin ? "Close the group" : "Leave the group", role: .destructive) {
                // The chat closes itself once the group is gone (ChatView), this screen with it.
                if let e = model.leaveGroup(gid) { error = e }
            }
        } message: {
            Text(isAdmin
                 ? "Every member stops receiving it. The messages already received stay on each phone."
                 : "You stop receiving its messages and your location shares with it stop. What you received stays on this phone.")
        }
        .confirmationDialog("Remove from the group?", isPresented: Binding(get: { removing != nil }, set: { if !$0 { removing = nil } }),
                            titleVisibility: .visible, presenting: removing) { m in
            Button("Remove \(m.name)", role: .destructive) { error = model.removeMember(m.key, from: gid) }
        } message: { _ in
            Text("Everyone gets new keys: they cannot read what follows.")
        }
        .onAppear {
            consent = model.searchConsent(gid)
            #if DEBUG
            if model.debugScreen == "invite", isAdmin { showingCode = true }
            #endif
        }
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
        // Alone in it (a group just created, to invite in person): no keys to wait for.
        if others == 0 {
            EmptyView()
        } else if Int(g.keysKnown) >= others {
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
                Text(me ? String(localized: "You") : m.name).font(.system(.callout))
                Text(admin ? String(localized: "Admin") : String(localized: "Member"))
                    .font(.system(.footnote)).foregroundStyle(Theme.tx2)
            }
            Spacer()
            if !me {
                if m.verified {
                    Badge(text: String(localized: "verified"), background: Theme.okBg, foreground: Theme.okFg)
                } else {
                    Badge(text: String(localized: "to verify"), background: Theme.warnBg, foreground: Theme.warnFg)
                }
            }
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 10)
        .contextMenu {
            if !me {
                Button {
                    if let e = model.startSearch(m.key) {
                        error = e
                    } else {
                        model.tab = .search
                    }
                } label: {
                    Label("Search for \(m.name)", systemImage: "scope")
                }
            }
            if isAdmin && !me {
                Button(role: .destructive) {
                    removing = m
                } label: {
                    Label("Remove from the group", systemImage: "person.badge.minus")
                }
            }
        }
    }
}

/// Invite contacts into the group (the admin): people whose identity is known, not yet in it.
private struct InviteSheet: View {
    let gid: Data
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    @State private var picked: Set<Data> = []
    @State private var error: String?

    var body: some View {
        let members = Set(model.groups.first { $0.gid == gid }?.members.map(\.key) ?? [])
        let candidates = model.contacts.filter { $0.identity != nil && !members.contains($0.key) }
        NavigationStack {
            List {
                Section {
                    ForEach(candidates, id: \.key) { c in
                        Button {
                            if picked.contains(c.key) { picked.remove(c.key) } else { picked.insert(c.key) }
                        } label: {
                            HStack {
                                Avatar(name: c.name, key: c.key, size: 36)
                                Text(c.name).foregroundStyle(Theme.tx)
                                Spacer()
                                if picked.contains(c.key) { Image(systemName: "checkmark").foregroundStyle(Theme.tx) }
                            }
                        }
                        .accessibilityAddTraits(picked.contains(c.key) ? .isSelected : [])
                    }
                } footer: {
                    Text(candidates.isEmpty
                         ? String(localized: "Contacts appear here once their identity is known: scan their code or exchange a message.")
                         : String(localized: "New members get the current keys: they can read messages recorded before they joined."))
                }
                if let error { Section { Text(error).foregroundStyle(Theme.sosFg) } }
            }
            .navigationTitle("Invite")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Invite") {
                        error = model.addMembers(Array(picked), to: gid)
                        if error == nil { dismiss() }
                    }
                    .disabled(picked.isEmpty)
                }
            }
        }
    }
}

/// The group's invitation as a QR code (the admin, protocol.md §10.5): people next to them scan
/// it and join while it is on screen, for 10 minutes. Closing it revokes the code.
private struct InviteCodeSheet: View {
    let gid: Data
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    @State private var code: InviteCode?
    @State private var error: String?
    /// Members when the sheet opened: the others joined with this code.
    @State private var before: Set<Data> = []

    var body: some View {
        let group = model.groups.first { $0.gid == gid }
        let joined = (group?.members ?? []).filter { !before.contains($0.key) }
        NavigationStack {
            ScrollView {
                VStack(spacing: 16) {
                    if let code, let image = QRCode.image(code.text) {
                        TimelineView(.periodic(from: .now, by: 1)) { ctx in
                            let left = Int(code.expires) - Int(ctx.date.timeIntervalSince1970)
                            VStack(spacing: 14) {
                                Image(uiImage: image)
                                    .interpolation(.none)
                                    .resizable()
                                    .scaledToFit()
                                    .padding(14)
                                    .background(.white, in: RoundedRectangle(cornerRadius: 16))
                                    .frame(maxWidth: 280)
                                    .opacity(left > 0 ? 1 : 0.15)
                                    .accessibilityLabel(String(localized: "Invitation code for \(group?.name ?? "")"))
                                if left > 0 {
                                    Text("Valid for \(left / 60):\(String(format: "%02d", left % 60))")
                                        .font(Theme.figure(17))
                                        .foregroundStyle(Theme.tx)
                                        .monospacedDigit()
                                } else {
                                    Banner(kind: .warn, title: String(localized: "This code has expired"),
                                           action: (String(localized: "New code"), { make() }))
                                }
                            }
                        }
                        Text("In lichen: Add a contact › Scan. They join \(group?.name ?? "") and you become a verified contact for them.")
                            .font(.system(.subheadline))
                            .foregroundStyle(Theme.tx2)
                            .multilineTextAlignment(.center)
                    }
                    if let error { Banner(kind: .sos, title: error) }
                    if !joined.isEmpty {
                        VStack(alignment: .leading, spacing: 8) {
                            SectionLabel("Joined")
                            CardList {
                                ForEach(Array(joined.enumerated()), id: \.element.key) { i, m in
                                    if i > 0 { CardDivider(inset: 68) }
                                    HStack(spacing: 12) {
                                        Avatar(name: m.name, key: m.key, size: 40)
                                        Text(m.name).font(.system(.callout))
                                        Spacer()
                                        Image(systemName: "checkmark.circle.fill").foregroundStyle(Theme.okFg)
                                    }
                                    .padding(.horizontal, 16)
                                    .padding(.vertical, 10)
                                }
                            }
                        }
                    }
                    Text("Anyone who scans it while it is shown can join: you can remove them from the group. Newcomers can read messages recorded under the current keys.")
                        .font(.system(.footnote))
                        .foregroundStyle(Theme.tx2)
                        .fixedSize(horizontal: false, vertical: true)
                }
                .padding(16)
            }
            .screen()
            .navigationTitle(group?.name ?? String(localized: "Invite"))
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .confirmationAction) { Button("Done") { dismiss() } }
            }
        }
        .onAppear {
            before = Set(group?.members.map(\.key) ?? [])
            make()
        }
        .onDisappear { if let code { model.revokeInvite(code.token) } }
    }

    /// A new code; the previous one is revoked.
    private func make() {
        if let code { model.revokeInvite(code.token) }
        switch model.createInvite(gid) {
        case let .success(c):
            code = c
            error = nil
        case let .failure(e):
            error = e.display
        }
    }
}
