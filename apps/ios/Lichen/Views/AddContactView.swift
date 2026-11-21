import AVFoundation
import CoreImage.CIFilterBuiltins
import SwiftUI

/// "Add a contact" (mockup `contact`, ADR-0013 §4.7): show my code, scan theirs, compare the
/// safety number.
struct AddContactView: View {
    /// In the first-launch flow: what "Done" does; otherwise the sheet closes.
    var next: (() -> Void)?
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    @State private var tab = Tab.myCode
    @State private var added: ContactInfo?
    /// Added from a code scanned in person: then they scan ours (B-user-1).
    @State private var addedInPerson = false
    @State private var showingMyCode = false
    @State private var invite: ScannedInvite?
    /// A MeshCore contact link (F-2): its card, to add it to the radio.
    @State private var link: ScannedLink?
    @State private var error: String?

    enum Tab: Hashable { case myCode, scan }

    var body: some View {
        VStack(spacing: 16) {
            if invite == nil, link == nil {
                Picker("Mode", selection: $tab) {
                    Text("My code").tag(Tab.myCode)
                    Text("Scan").tag(Tab.scan)
                }
                .pickerStyle(.segmented)
            }

            if let invite {
                JoinPanel(text: invite.text, info: invite.info, inPerson: invite.inPerson) { finish() }
            } else if let link {
                MeshCoreLinkPanel(text: link.text, link: link.info, open: { chat in
                    if next == nil { model.openRequest = chat }
                    finish()
                }, cancel: { self.link = nil }, done: finish)
            } else if let added, showingMyCode {
                ShowYourCode(name: added.name) { openChat(added) }
            } else if let added {
                VerifyPanel(node: added.key) {
                    // Scanned in person and the numbers match: now they scan ours, so that both
                    // phones verify each other in the same sitting (B-user-1).
                    if addedInPerson, model.contact(added.key)?.verified == true, model.myCard() != nil {
                        showingMyCode = true
                    } else {
                        openChat(added)
                    }
                }
            } else if tab == .myCode {
                myCode
            } else {
                ScanPanel { text, inPerson in scanned(text, inPerson: inPerson) }
            }
            if let error {
                Banner(kind: .sos, title: error)
                    // Its own height, not the screen's (a spacer follows).
                    .fixedSize(horizontal: false, vertical: true)
            }
            Spacer(minLength: 0)
            if added == nil, invite == nil, link == nil {
                Button(next == nil ? String(localized: "Close") : String(localized: "Later")) { finish() }
                    .buttonStyle(.secondary)
            }
        }
        .padding(16)
        .screen()
        .navigationTitle(invite == nil ? String(localized: "Add a contact") : String(localized: "Join a group"))
        .navigationBarTitleDisplayMode(.inline)
        .onAppear {
            #if DEBUG
            if let text = model.debugInvite {
                model.debugInvite = nil
                tab = .scan
                scanned(text, inPerson: true)
            }
            #endif
        }
    }

    /// A contact card adds the contact (verified only when scanned in person); a group
    /// invitation asks to join (§10.5).
    private func scanned(_ text: String, inPerson: Bool) {
        error = nil
        // Our own card or invitation, scanned or pasted back (D-3).
        if model.isOwnCode(text) {
            error = String(localized: "This is your own code.")
            return
        }
        addedInPerson = inPerson
        // A MeshCore contact link (F-2, ADR-0047): the stock app's signed advert, or a
        // contact code. Its card first; nothing reaches the radio before "Add to the radio".
        if text.trimmingCharacters(in: .whitespacesAndNewlines).lowercased().hasPrefix("meshcore:") {
            switch model.readMeshCoreLink(text) {
            case let .success(l) where l.key == model.nodeKey:
                error = String(localized: "This is your own code.")
            case let .success(l):
                link = ScannedLink(text: text, info: l)
            case .failure(.BadCode):
                error = String(localized: "This MeshCore link could not be read.")
            case let .failure(e):
                error = e.display
            }
            return
        }
        if text.trimmingCharacters(in: .whitespacesAndNewlines).hasPrefix("lichen:1:g:") {
            switch model.parseInvite(text) {
            case let .success(info): invite = ScannedInvite(text: text, info: info, inPerson: inPerson)
            case let .failure(e):
                error = e == .BadCode ? String(localized: "This invitation could not be read.") : e.display
            }
            return
        }
        switch model.addCard(text, scanned: inPerson) {
        case let .success(c): added = c
        case let .failure(e): error = e.display
        }
    }

    private func finish() {
        if let next { next() } else { dismiss() }
    }

    /// Their conversation opens, ready for the first message (the list shows contacts once
    /// there is a message).
    private func openChat(_ contact: ContactInfo) {
        if next == nil { model.openRequest = .contact(key: contact.key) }
        finish()
    }

    @ViewBuilder private var myCode: some View {
        if let card = model.myCard(), let image = QRCode.image(card) {
            VStack(spacing: 12) {
                Image(uiImage: image)
                    .interpolation(.none)
                    .resizable()
                    .scaledToFit()
                    .padding(14)
                    .background(.white, in: RoundedRectangle(cornerRadius: 16))
                    .frame(maxWidth: 240)
                    .accessibilityLabel("Your contact code")
                // The name the card carries, as the contact will see it.
                Text(verbatim: model.myName.isEmpty ? model.nodeName : model.myName)
                    .font(.system(.headline))
                Text("Have your contact scan this code, in person.")
                    .font(.system(.subheadline))
                    .foregroundStyle(Theme.tx2)
                    .multilineTextAlignment(.center)
            }
        } else {
            Banner(kind: .warn, title: String(localized: "Connect your radio first"),
                   message: String(localized: "Your code carries your radio’s address, so people can reach you."))
                // Its own height, not the screen's (a spacer follows).
                .fixedSize(horizontal: false, vertical: true)
        }
    }
}

/// After we scanned their code in person: ours, for them to scan, so that their phone verifies
/// us too (B-user-1).
private struct ShowYourCode: View {
    let name: String
    let done: () -> Void
    @Environment(AppModel.self) private var model

    var body: some View {
        VStack(spacing: 12) {
            Text("Now let \(name) scan your code")
                .font(.system(.title3, weight: .semibold))
                .multilineTextAlignment(.center)
            Text("Their phone verifies you the same way, and both of you see the same numbers.")
                .font(.system(.subheadline))
                .foregroundStyle(Theme.tx2)
                .multilineTextAlignment(.center)
                .fixedSize(horizontal: false, vertical: true)
            if let card = model.myCard(), let image = QRCode.image(card) {
                Image(uiImage: image)
                    .interpolation(.none)
                    .resizable()
                    .scaledToFit()
                    .padding(14)
                    .background(.white, in: RoundedRectangle(cornerRadius: 16))
                    .frame(maxWidth: 240)
                    .accessibilityLabel("Your contact code")
            }
            Button("Done", action: done)
                .buttonStyle(.primary)
                .padding(.top, 4)
        }
        .frame(maxWidth: .infinity)
    }
}

private struct ScannedLink {
    let text: String
    let info: MeshCoreLink
}

/// A MeshCore contact link, read (F-2, ADR-0047): the node it names, its kind and key, and
/// whether its own radio signed it; then "Add to the radio", and what the radio did.
private struct MeshCoreLinkPanel: View {
    let text: String
    let link: MeshCoreLink
    let open: (ChatID) -> Void
    /// Back to the scanner, nothing sent.
    let cancel: () -> Void
    /// Add a contact closes.
    let done: () -> Void
    @Environment(AppModel.self) private var model
    @State private var step = Step.ready
    /// `model.linkRefusals` when the link went to the radio: one more is its refusal.
    @State private var refusalsBefore = 0
    @State private var error: String?

    enum Step { case ready, adding, added, already }

    /// On the radio: among the people, or under Nodes for a repeater, room server or sensor.
    private var onRadio: Bool {
        model.coreContacts.contains { $0.key == link.key } || model.nodes.contains { $0.key == link.key && $0.onRadio }
    }

    private var refused: Bool { step == .adding && model.linkRefusals > refusalsBefore }

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            VStack(alignment: .leading, spacing: 0) {
                HStack(spacing: 14) {
                    Avatar(name: link.name, key: link.key, size: 48)
                    VStack(alignment: .leading, spacing: 4) {
                        Text(verbatim: link.name).font(.system(.title3, weight: .semibold))
                        Label(link.kind.label, systemImage: link.kind.symbol)
                            .font(.system(.subheadline)).foregroundStyle(Theme.tx2)
                    }
                    Spacer(minLength: 0)
                }
                .padding(16)
                Divider().overlay(Theme.line)
                MeshCoreKeyRow(key: link.key)
                Divider().overlay(Theme.line)
                Label(link.signed ? String(localized: "Signed by its radio.")
                                  : String(localized: "Not signed: anyone can write such a code."),
                      systemImage: link.signed ? "checkmark.seal" : "exclamationmark.triangle")
                    .font(.system(.footnote))
                    .foregroundStyle(link.signed ? Theme.ok : Theme.tx2)
                    .fixedSize(horizontal: false, vertical: true)
                    .padding(.horizontal, 16)
                    .padding(.vertical, 11)
                // How the advert reached the radio that shared it: signed adverts only (a
                // contact code says nothing of it).
                if link.signed {
                    Divider().overlay(Theme.line)
                    Label(link.hops == 0 ? String(localized: "Heard by the sharer’s radio: directly")
                                         : String(localized: "Heard by the sharer’s radio: through \(Int(link.hops)) repeaters"),
                          systemImage: "antenna.radiowaves.left.and.right")
                        .font(.system(.footnote))
                        .foregroundStyle(Theme.tx2)
                        .fixedSize(horizontal: false, vertical: true)
                        .padding(.horizontal, 16)
                        .padding(.vertical, 11)
                }
            }
            .background(Theme.s1, in: RoundedRectangle(cornerRadius: 14))
            if refused {
                Banner(kind: .sos, title: String(localized: "The radio did not add it."))
                Button("Close", action: done).buttonStyle(.secondary)
            } else {
                switch step {
                case .ready:
                    Button("Add to the radio", action: add).buttonStyle(.primary)
                    Button("Cancel", action: cancel).buttonStyle(.secondary)
                case .adding:
                    HStack(spacing: 10) {
                        ProgressView()
                        Text("Adding to your radio…").font(.system(.callout, weight: .medium))
                    }
                case .added, .already:
                    Banner(kind: .ok, title: step == .added ? String(localized: "Added to your radio.")
                                                             : String(localized: "Already on your radio."))
                    if link.kind == .companion {
                        Button("Open the chat") { openChat() }.buttonStyle(.primary)
                        Button("Close", action: done).buttonStyle(.secondary)
                    } else {
                        Button("Done", action: done).buttonStyle(.primary)
                    }
                }
            }
            if let error { Banner(kind: .sos, title: error) }
        }
        // Banners take their own height, not the screen's.
        .fixedSize(horizontal: false, vertical: true)
        .onChange(of: onRadio) { _, now in if now, step == .adding { step = .added } }
    }

    private func add() {
        error = nil
        refusalsBefore = model.linkRefusals
        switch model.addMeshCoreLink(text) {
        case .success(.sent): step = onRadio ? .added : .adding
        case .success(.alreadyOnTheRadio): step = .already
        case .success(.ownRadio): error = String(localized: "This is your own code.")
        case .failure(.NoIdentity): error = String(localized: "Connect your radio first")
        case let .failure(e): error = e.display
        }
    }

    /// Their chat: the MeshCore one, or lichen's once their identity is known.
    private func openChat() {
        let lichen = model.coreContacts.first { $0.key == link.key }?.identity != nil
        open(lichen ? .contact(key: link.key) : .mesh(key: link.key))
    }
}

private struct ScannedInvite {
    let text: String
    let info: InviteInfo
    /// Scanned with the camera, not pasted: only then does it verify the admin.
    let inPerson: Bool
}

/// A scanned group invitation (§10.5): what it is, then the request to the admin until the
/// group arrives.
private struct JoinPanel: View {
    let text: String
    let info: InviteInfo
    let inPerson: Bool
    let done: () -> Void
    @Environment(AppModel.self) private var model
    @State private var asked = false
    @State private var error: String?

    var body: some View {
        let admin = info.adminName.isEmpty ? String(localized: "the admin") : info.adminName
        let name = info.groupName.isEmpty ? String(localized: "the group") : info.groupName
        VStack(alignment: .leading, spacing: 14) {
            HStack(spacing: 14) {
                Avatar(name: name, key: info.admin, size: 48)
                VStack(alignment: .leading, spacing: 4) {
                    Text("Join \(name)?").font(.system(.title3, weight: .semibold))
                    Text("Invitation from \(admin)")
                        .font(.system(.subheadline)).foregroundStyle(Theme.tx2)
                }
                Spacer(minLength: 0)
            }
            .padding(16)
            .background(Theme.s1, in: RoundedRectangle(cornerRadius: 14))
            if let gid = model.joinedByInvite[info.admin] {
                #if DEBUG
                // -joinNow YES: the group opens once joined (screenshots).
                Color.clear.frame(height: 0).onAppear {
                    if UserDefaults.standard.bool(forKey: "joinNow") {
                        model.openRequest = .group(gid: gid)
                        done()
                    }
                }
                #endif
                Banner(kind: .ok, title: String(localized: "You joined \(name)"),
                       message: String(localized: "\(admin) added you. The other members’ keys arrive over the next minutes."))
                Button("Open the group") {
                    model.openRequest = .group(gid: gid)
                    done()
                }
                .buttonStyle(.primary)
            } else if asked {
                HStack(spacing: 10) {
                    ProgressView()
                    Text("Asking \(admin) to add you…").font(.system(.callout, weight: .medium))
                }
                Text("Their phone adds you over LoRa, usually within a minute or two. You can close this screen: the group appears in your conversations.")
                    .font(.system(.footnote)).foregroundStyle(Theme.tx2)
                    .fixedSize(horizontal: false, vertical: true)
                Button("Close") { done() }.buttonStyle(.secondary)
            } else if info.member {
                Banner(kind: .ok, title: String(localized: "You are already in \(name)"))
                Button("Close") { done() }.buttonStyle(.secondary)
            } else if info.expired {
                Banner(kind: .warn, title: String(localized: "This invitation has expired"),
                       message: String(localized: "Ask \(admin) to show a new code."))
                Button("Close") { done() }.buttonStyle(.secondary)
            } else {
                Text(inPerson
                     ? String(localized: "\(admin) becomes a verified contact: you scanned their code in person. You read what the group sends from now on, not what came before.")
                     : String(localized: "Pasted, not scanned: \(admin) is added without being verified. Compare safety numbers with them to verify. You read what the group sends from now on, not what came before."))
                    .font(.system(.footnote)).foregroundStyle(Theme.tx2)
                    .fixedSize(horizontal: false, vertical: true)
                Button("Join") { join() }
                    .buttonStyle(.primary)
                Button("Cancel") { done() }.buttonStyle(.secondary)
            }
            if let error { Banner(kind: .sos, title: error) }
        }
        // Banners take their own height, not the screen's.
        .fixedSize(horizontal: false, vertical: true)
        .onAppear {
            #if DEBUG
            // -joinNow YES: as if "Join" were tapped.
            if UserDefaults.standard.bool(forKey: "joinNow"), !asked, !info.member, !info.expired { join() }
            #endif
        }
    }

    private func join() {
        error = model.acceptInvite(text, info, scanned: inPerson)
        if error == nil { asked = true }
    }
}

/// The safety number with a contact (§4.7): the same 60 digits on both phones. Approving
/// names the identity whose number was shown, so a key that changes meanwhile is never
/// approved unseen (review PR-07).
struct VerifyPanel: View {
    let node: Data
    /// A new identity the contact presented, compared before accepting it.
    var pending: Data?
    let done: () -> Void
    @Environment(AppModel.self) private var model
    /// The identity whose number is on screen, fixed when the panel opens.
    @State private var shown: Data?
    @State private var isNew = false
    @State private var stale = false

    var body: some View {
        let name = model.contact(node)?.name ?? model.name(of: node)
        let numbers = shown.flatMap { model.safetyNumber(withIdentity: $0) }
        VStack(alignment: .leading, spacing: 14) {
            if stale {
                Banner(kind: .warn, title: String(localized: "Their key changed again"),
                       message: String(localized: "Compare the new numbers below before approving."))
            } else if model.contact(node)?.verified == true, !isNew {
                Banner(kind: .ok, title: String(localized: "\(name) added, verified"),
                       message: String(localized: "You scanned their code in person."))
            }
            VStack(alignment: .leading, spacing: 12) {
                Text("Compare these numbers with \(name)")
                    .font(.system(.callout, weight: .semibold))
                if let numbers {
                    SafetyNumber(groups: numbers).frame(maxWidth: .infinity)
                } else {
                    Text("Their key is not known yet: exchange a message first, or scan their code.")
                        .font(.system(.subheadline)).foregroundStyle(Theme.tx2)
                }
                Text("The same on both phones: nobody in between. Compare them aloud, or over another channel.")
                    .font(.system(.footnote))
                    .foregroundStyle(Theme.tx2)
                    .fixedSize(horizontal: false, vertical: true)
            }
            .padding(16)
            .background(Theme.s1, in: RoundedRectangle(cornerRadius: 14))
            // Already verified (a scanned code): nothing to approve; the numbers are a check.
            let verified = model.contact(node)?.verified == true && !isNew && !stale
            if numbers != nil, verified {
                Text("Verified by the scan. Their phone shows the same numbers once it knows you: after your first message, or when they scan your code.")
                    .font(.system(.footnote))
                    .foregroundStyle(Theme.tx2)
                    .fixedSize(horizontal: false, vertical: true)
                Button("Done") { done() }
                    .buttonStyle(.primary)
            }
            if numbers != nil, let shown, !verified {
                Button("The numbers match") {
                    let ok = isNew ? model.acceptIdentity(node, expected: shown)
                                   : model.setVerified(node, identity: shown, true)
                    if ok { done() } else { refresh(stale: true) }
                }
                .buttonStyle(.primary)
                Button {
                    if isNew { model.rejectIdentity(node, expected: shown) } else { model.setVerified(node, identity: shown, false) }
                    done()
                } label: {
                    Text("They differ")
                        .font(.system(.callout, weight: .semibold))
                        .frame(maxWidth: .infinity, minHeight: 50)
                        .background(Theme.sosBg, in: RoundedRectangle(cornerRadius: 14))
                        .foregroundStyle(Theme.sosFg)
                }
            }
            if verified, let shown {
                Button("They differ") {
                    model.setVerified(node, identity: shown, false)
                    done()
                }
                .font(.system(.subheadline, weight: .semibold))
                .foregroundStyle(Theme.sosFg)
                .frame(maxWidth: .infinity, minHeight: 44)
            }
        }
        // Banners take their own height, not the screen's (Add a contact has a spacer below).
        .fixedSize(horizontal: false, vertical: true)
        .onAppear { if shown == nil { refresh(stale: false) } }
    }

    /// Fixes the identity on screen: the pending new one if any, else the contact's own.
    private func refresh(stale: Bool) {
        let newer = model.conflicts[node] ?? (stale ? nil : pending)
        isNew = newer != nil
        shown = newer ?? model.contact(node)?.identity
        self.stale = stale
    }
}

/// The verification sheet opened from a conversation.
struct VerifySheet: View {
    let node: Data
    var pending: Data?
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 14) {
                    VerifyPanel(node: node, pending: pending) { dismiss() }
                    // Their radio's MeshCore public key (F-1).
                    CardList { MeshCoreKeyRow(key: node) }
                }
                .padding(16)
            }
            .screen()
            .navigationTitle(pending == nil ? "Safety number" : "New key")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Close") { dismiss() } }
            }
        }
    }
}

/// Scans a contact code with the camera; without one (simulator), a field to paste it.
private struct ScanPanel: View {
    /// The code, and whether the camera read it (in person) rather than a paste.
    let found: (String, Bool) -> Void
    @State private var pasted = ""
    @State private var cameraOK = AVCaptureDevice.default(for: .video) != nil

    /// A lichen code, or a MeshCore contact link (F-2).
    static func isCode(_ text: String) -> Bool {
        let t = text.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        return t.hasPrefix("lichen:") || t.hasPrefix("meshcore://")
    }

    var body: some View {
        VStack(spacing: 12) {
            if cameraOK {
                QRScanner(prefixes: ["lichen:", "meshcore://"]) { code in found(code, true) }
                    .frame(height: 300)
                    .clipShape(RoundedRectangle(cornerRadius: 16))
                    .overlay(RoundedRectangle(cornerRadius: 16).stroke(Theme.line))
                Text("Point the camera at your contact’s code.")
                    .font(.system(.subheadline)).foregroundStyle(Theme.tx2)
            }
            HStack {
                TextField("Or paste a code: lichen:1:…", text: $pasted)
                    .accessibilityIdentifier("pasteCode")
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
                    .font(.system(.subheadline, design: .monospaced))
                    .padding(10)
                    .background(Theme.s1, in: RoundedRectangle(cornerRadius: 10))
                Button("Add") { found(pasted, false) }
                    .disabled(!Self.isCode(pasted))
            }
        }
    }
}

/// QR codes from CoreImage.
enum QRCode {
    static func image(_ text: String) -> UIImage? {
        let f = CIFilter.qrCodeGenerator()
        f.message = Data(text.utf8)
        f.correctionLevel = "M"
        guard let out = f.outputImage?.transformed(by: CGAffineTransform(scaleX: 8, y: 8)),
              let cg = CIContext().createCGImage(out, from: out.extent)
        else { return nil }
        return UIImage(cgImage: cg)
    }
}

/// The camera, reporting the first code it reads that starts with one of `prefixes` (lichen
/// codes by default; a MeshCore channel's for channels, protocol.md §15.3).
struct QRScanner: UIViewControllerRepresentable {
    var prefixes = ["lichen:"]
    let found: (String) -> Void

    func makeUIViewController(context: Context) -> ScannerController {
        let c = ScannerController()
        c.found = found
        c.prefixes = prefixes
        return c
    }

    func updateUIViewController(_ c: ScannerController, context: Context) {}

    final class ScannerController: UIViewController, AVCaptureMetadataOutputObjectsDelegate {
        var found: ((String) -> Void)?
        var prefixes = ["lichen:"]
        private let session = AVCaptureSession()
        private var reported = false

        override func viewDidLoad() {
            super.viewDidLoad()
            view.backgroundColor = .black
            guard let device = AVCaptureDevice.default(for: .video),
                  let input = try? AVCaptureDeviceInput(device: device),
                  session.canAddInput(input)
            else { return }
            session.addInput(input)
            let output = AVCaptureMetadataOutput()
            guard session.canAddOutput(output) else { return }
            session.addOutput(output)
            output.setMetadataObjectsDelegate(self, queue: .main)
            output.metadataObjectTypes = [.qr]
            let layer = AVCaptureVideoPreviewLayer(session: session)
            layer.videoGravity = .resizeAspectFill
            layer.frame = view.bounds
            view.layer.addSublayer(layer)
        }

        override func viewDidLayoutSubviews() {
            super.viewDidLayoutSubviews()
            view.layer.sublayers?.first?.frame = view.bounds
        }

        override func viewWillAppear(_ animated: Bool) {
            super.viewWillAppear(animated)
            let s = session
            DispatchQueue.global(qos: .userInitiated).async { s.startRunning() }
        }

        override func viewWillDisappear(_ animated: Bool) {
            super.viewWillDisappear(animated)
            session.stopRunning()
        }

        nonisolated func metadataOutput(_ output: AVCaptureMetadataOutput, didOutput objects: [AVMetadataObject],
                                        from connection: AVCaptureConnection) {
            guard let code = (objects.first as? AVMetadataMachineReadableCodeObject)?.stringValue else { return }
            MainActor.assumeIsolated {
                guard !reported, prefixes.contains(where: { code.lowercased().hasPrefix($0) }) else { return }
                reported = true
                UINotificationFeedbackGenerator().notificationOccurred(.success)
                found?(code)
            }
        }
    }
}
