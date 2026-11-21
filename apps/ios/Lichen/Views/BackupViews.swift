import SwiftUI
import UniformTypeIdentifiers

/// Settings › Backup and the identity screen (ADR-0037): makes the encrypted backup, then lets
/// the user save it in Files or send it. The file waits in the app's temporary folder until then.
struct BackupExportSheet: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    @State private var outcome: AppModel.BackupOutcome?
    @State private var saving = false
    @State private var saved = false

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 16) {
                    Text("One file with your messages, contacts, groups, settings and media. Only your 24 words open it: keep both, but not together.")
                        .font(.system(.subheadline))
                        .foregroundStyle(Theme.tx2)
                        .fixedSize(horizontal: false, vertical: true)
                    switch outcome {
                    case nil:
                        HStack(spacing: 10) {
                            ProgressView()
                            Text("Preparing the backup…").font(.system(.callout))
                        }
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .card(padding: 16)
                    case let .made(url, summary)?:
                        ready(url, summary)
                    case let .failed(why)?:
                        Banner(kind: .sos, title: String(localized: "No backup was made"), message: why)
                        Button("Try again") { start() }
                            .buttonStyle(.secondary)
                    }
                    Text("A backup leaves out disappearing messages, what was still waiting to be sent, and location sharing. After a restore, your contacts and groups see you as after a restart.")
                        .font(.system(.footnote))
                        .foregroundStyle(Theme.tx2)
                        .fixedSize(horizontal: false, vertical: true)
                }
                .padding(16)
            }
            .screen()
            .navigationTitle("Encrypted backup")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button(saved ? "Done" : "Close") { dismiss() } }
            }
        }
        .task { if outcome == nil { start() } }
        .onDisappear {
            if case let .made(url, _)? = outcome { try? FileManager.default.removeItem(at: url) }
        }
    }

    private func start() {
        outcome = nil
        Task { outcome = await model.exportBackup() }
    }

    @ViewBuilder
    private func ready(_ url: URL, _ s: BackupSummary) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            Label(saved ? String(localized: "Backup saved") : String(localized: "Backup ready"),
                  systemImage: "checkmark.shield")
                .font(.system(.callout, weight: .semibold))
                .labelStyle(TintedIconLabel(tint: Theme.ok))
            Text("\(Int(s.messages)) messages, \(Int(s.files)) media files, \(ByteCountFormatter.string(fromByteCount: Int64(s.bytes), countStyle: .file))")
                .font(.system(.footnote).monospacedDigit())
                .foregroundStyle(Theme.tx2)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .card(padding: 16)
        if !saved {
            Button("Save to Files") {
                #if DEBUG
                // -saveBackup <name>: saved as `name` in this app's temporary folder, as if put in
                // Files (UI tests; -restoreFile picks it there).
                if let name = UserDefaults.standard.string(forKey: "saveBackup") {
                    let to = FileManager.default.temporaryDirectory.appendingPathComponent(name)
                    try? FileManager.default.removeItem(at: to)
                    saved = (try? FileManager.default.moveItem(at: url, to: to)) != nil
                    return
                }
                #endif
                saving = true
            }
                .buttonStyle(.primary)
                .fileMover(isPresented: $saving, file: url) { result in
                    if case .success = result { saved = true }
                }
            ShareLink(item: url) {
                Text("Send it somewhere else…").frame(maxWidth: .infinity)
            }
            .buttonStyle(.secondary)
        }
    }
}

/// "Restore a backup" (ADR-0037), at first launch and on the identity screen: the file, then its
/// 24 words. The backup's identity and data replace this phone's.
struct RestoreBackupSheet: View {
    /// Run once the backup is restored.
    var restored: (() -> Void)?
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    @State private var file: URL?
    @State private var picking = false
    @State private var text = ""
    @State private var working = false
    @State private var error: String?

    private var words: [String] {
        text.split(whereSeparator: { $0.isWhitespace || $0 == "," }).map(String.init)
    }


    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 14) {
                    Text("Your messages, contacts, groups, settings and media come back from a backup file. It replaces the identity and the messages on this phone.")
                        .font(.system(.subheadline))
                        .foregroundStyle(Theme.tx2)
                        .fixedSize(horizontal: false, vertical: true)
                    CardList {
                        Button {
                            #if DEBUG
                            // -restoreFile <name>: this backup of the app's temporary folder (where
                            // -saveBackup puts one), as if picked in Files (UI tests).
                            if let name = UserDefaults.standard.string(forKey: "restoreFile") {
                                file = FileManager.default.temporaryDirectory.appendingPathComponent(name)
                                return
                            }
                            #endif
                            picking = true
                        } label: {
                            CardRow(title: file?.lastPathComponent ?? String(localized: "Choose the backup file"),
                                    subtitle: file == nil ? String(localized: "From Files, iCloud Drive or a USB drive") : nil) {
                                Image(systemName: file == nil ? "doc.badge.plus" : "checkmark.circle.fill")
                                    .foregroundStyle(file == nil ? Theme.tx2 : Theme.ok)
                            }
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                        .disabled(working)
                    }
                    Text("The 24 words of this identity").font(.system(.callout, weight: .semibold))
                    TextEditor(text: $text)
                        .font(.system(.callout, design: .monospaced))
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                        .scrollContentBackground(.hidden)
                        .padding(8)
                        .frame(minHeight: 140)
                        .background(Theme.s1, in: RoundedRectangle(cornerRadius: 12))
                        .overlay(RoundedRectangle(cornerRadius: 12).stroke(Theme.line))
                        .disabled(working)
                    Text("\(words.count) / 24 words")
                        .font(.system(.footnote).monospacedDigit())
                        .foregroundStyle(words.count == 24 ? Theme.ok : Theme.tx2)
                    if let error {
                        Banner(kind: .sos, title: error)
                    }
                    Button {
                        guard let file else { return }
                        working = true
                        error = nil
                        Task {
                            error = await model.restoreBackup(from: file, words: words)
                            working = false
                            if error == nil {
                                dismiss()
                                restored?()
                            }
                        }
                    } label: {
                        HStack(spacing: 8) {
                            if working { ProgressView() }
                            Text(working ? String(localized: "Restoring…") : String(localized: "Restore"))
                        }
                    }
                    .buttonStyle(.primary)
                    .disabled(file == nil || words.count != 24 || working)
                    .padding(.top, 4)
                }
                .padding(16)
            }
            .screen()
            .secretCover()
            .navigationTitle("Restore a backup")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() }.disabled(working) }
            }
            .fileImporter(isPresented: $picking, allowedContentTypes: [.item]) { result in
                if case let .success(url) = result { file = url }
            }
            .interactiveDismissDisabled(working)
        }
    }
}
