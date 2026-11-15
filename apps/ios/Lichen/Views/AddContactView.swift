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
    @State private var invite: ScannedInvite?
    @State private var error: String?

    enum Tab: Hashable { case myCode, scan }

    var body: some View {
        VStack(spacing: 16) {
            if invite == nil {
                Picker("Mode", selection: $tab) {
                    Text("My code").tag(Tab.myCode)
                    Text("Scan").tag(Tab.scan)
                }
                .pickerStyle(.segmented)
            }

            if let invite {
                JoinPanel(text: invite.text, info: invite.info) { finish() }
            } else if let added {
                VerifyPanel(node: added.key) {
                    // Their conversation opens, ready for the first message (the list shows
                    // contacts once there is a message).
                    if next == nil { model.debugOpen = .contact(key: added.key) }
                    finish()
                }
            } else if tab == .myCode {
                myCode
            } else {
                ScanPanel { text, inPerson in scanned(text, inPerson: inPerson) }
            }
            if let error {
                Banner(kind: .sos, title: error)
            }
            Spacer(minLength: 0)
            if added == nil, invite == nil {
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
        if text.trimmingCharacters(in: .whitespacesAndNewlines).hasPrefix("lichen:1:g:") {
            switch model.parseInvite(text) {
            case let .success(info): invite = ScannedInvite(text: text, info: info)
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
                Text("Have your contact scan this code, in person.")
                    .font(.system(.subheadline))
                    .foregroundStyle(Theme.tx2)
                    .multilineTextAlignment(.center)
            }
        } else {
            Banner(kind: .warn, title: String(localized: "Connect your radio first"),
                   message: String(localized: "Your code carries your radio's address, so people can reach you."))
        }
    }
}

private struct ScannedInvite {
    let text: String
    let info: InviteInfo
}

/// A scanned group invitation (§10.5): what it is, then the request to the admin until the
/// group arrives.
private struct JoinPanel: View {
    let text: String
    let info: InviteInfo
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
                        model.debugOpen = .group(gid: gid)
                        done()
                    }
                }
                #endif
                Banner(kind: .ok, title: String(localized: "You joined \(name)"),
                       message: String(localized: "\(admin) added you. The other members' keys arrive over the next minutes."))
                Button("Open the group") {
                    model.debugOpen = .group(gid: gid)
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
                Text("\(admin) becomes a verified contact: you scanned their code in person. Members can read what was sent under the group's current keys.")
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
        error = model.acceptInvite(text, info)
        if error == nil { asked = true }
    }
}

/// The safety number with a contact (§4.7): the same 30 digits on both phones. Approving
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
                VerifyPanel(node: node, pending: pending) { dismiss() }.padding(16)
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

    var body: some View {
        VStack(spacing: 12) {
            if cameraOK {
                QRScanner { code in found(code, true) }
                    .frame(height: 300)
                    .clipShape(RoundedRectangle(cornerRadius: 16))
                    .overlay(RoundedRectangle(cornerRadius: 16).stroke(Theme.line))
                Text("Point the camera at your contact's code.")
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
                    .disabled(!pasted.hasPrefix("lichen:"))
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

/// The camera, reporting the first lichen code it reads.
private struct QRScanner: UIViewControllerRepresentable {
    let found: (String) -> Void

    func makeUIViewController(context: Context) -> ScannerController {
        let c = ScannerController()
        c.found = found
        return c
    }

    func updateUIViewController(_ c: ScannerController, context: Context) {}

    final class ScannerController: UIViewController, AVCaptureMetadataOutputObjectsDelegate {
        var found: ((String) -> Void)?
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
            guard let code = (objects.first as? AVMetadataMachineReadableCodeObject)?.stringValue,
                  code.hasPrefix("lichen:")
            else { return }
            MainActor.assumeIsolated {
                guard !reported else { return }
                reported = true
                UINotificationFeedbackGenerator().notificationOccurred(.success)
                found?(code)
            }
        }
    }
}
