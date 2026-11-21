import SwiftUI

/// "Your identity" (mockup `identite`, ADR-0013): name, key fingerprint, backup of the
/// 24-word phrase, restore.
struct IdentityView: View {
    /// In the first-launch flow: what "Continue" does.
    var next: (() -> Void)?
    @Environment(AppModel.self) private var model
    @State private var showingPhrase = false
    @State private var restoring = false
    @State private var exporting = false
    @State private var restoringBackup = false

    var body: some View {
        @Bindable var model = model
        ScrollView {
            VStack(spacing: 16) {
                Avatar(name: model.myName.isEmpty ? model.nodeName : model.myName, key: model.core.key() ?? Data(), size: 76)
                    .padding(.top, 8)
                TextField("Your name", text: $model.myName)
                    .font(.system(.body, weight: .medium))
                    .multilineTextAlignment(.center)
                    .padding(.vertical, 12)
                    .frame(maxWidth: 240)
                    .background(Theme.s1, in: RoundedRectangle(cornerRadius: 12))
                    .overlay(RoundedRectangle(cornerRadius: 12).stroke(Theme.line))
                    .submitLabel(.done)

                VStack(alignment: .leading, spacing: 12) {
                    Label("Your key’s fingerprint", systemImage: "key")
                        .font(.system(.callout, weight: .semibold))
                        .labelStyle(TintedIconLabel(tint: Theme.ok))
                    KeyFingerprint(groups: model.fingerprint)
                        .frame(maxWidth: .infinity)
                    Text("An X25519 / Ed25519 key made on this phone. It never leaves it: your radio only carries the messages.")
                        .font(.system(.footnote))
                        .foregroundStyle(Theme.tx2)
                        .fixedSize(horizontal: false, vertical: true)
                }
                .padding(16)
                .background(Theme.s1, in: RoundedRectangle(cornerRadius: 14))

                if model.isDemo {
                    // The demo runs on a throwaway identity: its words are not the phone's, and
                    // writing them down must not mark the phone's identity as backed up.
                    Banner(kind: .iroh, title: String(localized: "The demo’s identity"),
                           message: String(localized: "The demo network uses an identity of its own, thrown away when you leave it. Yours, and its 24 words, are back then."))
                } else if !model.phraseBackedUp {
                    Banner(kind: .warn, title: String(localized: "Not backed up"),
                           message: String(localized: "Without a backup, losing this phone means losing your identity and your verified contacts."))
                }

                CardList {
                    if !model.isDemo {
                        Button { showingPhrase = true } label: {
                            CardRow(title: String(localized: "Recovery phrase"),
                                    subtitle: String(localized: "24 words to write on paper")) {
                                Image(systemName: "chevron.right").foregroundStyle(Theme.tx2)
                            }
                        }
                        .buttonStyle(.plain)
                        CardDivider()
                    }
                    Button { restoring = true } label: {
                        CardRow(title: String(localized: "Restore an identity"),
                                subtitle: String(localized: "From its 24 words: replaces this one")) {
                            Image(systemName: "chevron.right").foregroundStyle(Theme.tx2)
                        }
                    }
                    .buttonStyle(.plain)
                    CardDivider()
                    Button { restoringBackup = true } label: {
                        CardRow(title: String(localized: "Restore a backup"),
                                subtitle: String(localized: "A backup file and its 24 words: replaces this identity and its messages")) {
                            Image(systemName: "chevron.right").foregroundStyle(Theme.tx2)
                        }
                    }
                    .buttonStyle(.plain)
                    if !model.isDemo {
                        CardDivider()
                        Button { exporting = true } label: {
                            CardRow(title: String(localized: "Encrypted backup"),
                                    subtitle: String(localized: "Your messages, contacts, groups and media in one file your 24 words open")) {
                                Image(systemName: "chevron.right").foregroundStyle(Theme.tx2)
                            }
                        }
                        .buttonStyle(.plain)
                    }
                }

                if let next {
                    Button("Continue", action: next)
                        .buttonStyle(.primary)
                        .padding(.top, 8)
                }
            }
            .padding(16)
        }
        .screen()
        .navigationTitle("Your identity")
        .navigationBarTitleDisplayMode(.inline)
        .sheet(isPresented: $showingPhrase) { RecoveryPhraseSheet() }
        .sheet(isPresented: $restoring) { RestoreSheet() }
        .sheet(isPresented: $exporting) { BackupExportSheet() }
        .sheet(isPresented: $restoringBackup) { RestoreBackupSheet(restored: next) }
    }
}

/// The 24 words, to write on paper.
struct RecoveryPhraseSheet: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    /// The three positions asked for once the user says the words are written down (D-7).
    @State private var asked: [Int]?
    @State private var answers = ["", "", ""]
    @State private var wrong = false

    var body: some View {
        let words = model.recoveryPhrase()
        NavigationStack {
            ScrollView {
                if let asked {
                    confirm(words, asked)
                } else {
                    phrase(words)
                }
            }
            .screen()
            .secretCover()
            .navigationTitle(asked == nil ? "Recovery phrase" : "Check your phrase")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Close") { dismiss() } }
            }
        }
    }

    private func phrase(_ words: [String]) -> some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Write these 24 words on paper, in order, and keep them somewhere safe. Anyone who has them can be you.")
                .font(.system(.subheadline))
                .foregroundStyle(Theme.tx2)
                .fixedSize(horizontal: false, vertical: true)
            LazyVGrid(columns: [GridItem(.flexible()), GridItem(.flexible())], alignment: .leading, spacing: 10) {
                ForEach(Array(words.enumerated()), id: \.offset) { i, w in
                    HStack(spacing: 8) {
                        Text("\(i + 1)")
                            .font(.system(.footnote).monospacedDigit())
                            .foregroundStyle(Theme.tx2)
                            .frame(width: 22, alignment: .trailing)
                        Text(verbatim: w).font(.system(.body, design: .monospaced, weight: .semibold))
                            .accessibilityIdentifier("phrase.\(i + 1)")
                    }
                    .padding(.vertical, 6)
                    .padding(.horizontal, 10)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(Theme.s1, in: RoundedRectangle(cornerRadius: 10))
                }
            }
            .privacySensitive()
            // Written down: three of the words, at random, before it counts as a backup.
            Button("I wrote them down") {
                asked = Array((0..<words.count).shuffled().prefix(3)).sorted()
                answers = ["", "", ""]
                wrong = false
            }
            .buttonStyle(.primary)
            .padding(.top, 8)
        }
        .padding(16)
    }

    private func confirm(_ words: [String], _ asked: [Int]) -> some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Type these three words of your phrase, from your paper.")
                .font(.system(.subheadline))
                .foregroundStyle(Theme.tx2)
                .fixedSize(horizontal: false, vertical: true)
            ForEach(Array(asked.enumerated()), id: \.offset) { k, position in
                VStack(alignment: .leading, spacing: 6) {
                    Text("Word \(position + 1)").font(.system(.subheadline, weight: .semibold))
                    TextField(text: $answers[k]) { Text("Word \(position + 1)") }
                        .accessibilityIdentifier("confirm.\(k + 1)")
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                        .font(.system(.body, design: .monospaced))
                        .padding(10)
                        .background(Theme.s1, in: RoundedRectangle(cornerRadius: 10))
                        .overlay(RoundedRectangle(cornerRadius: 10).stroke(Theme.line))
                }
            }
            .privacySensitive()
            if wrong {
                Banner(kind: .sos, title: String(localized: "One of these words is not the one in your phrase."),
                       message: String(localized: "Look at the phrase again, and correct your paper if needed."))
            }
            Button("Check") {
                let right = zip(asked, answers).allSatisfy { position, answer in
                    words.indices.contains(position)
                        && answer.trimmingCharacters(in: .whitespaces).lowercased() == words[position].lowercased()
                }
                if right {
                    model.phraseBackedUp = true
                    dismiss()
                } else {
                    wrong = true
                }
            }
            .buttonStyle(.primary)
            .disabled(answers.contains { $0.trimmingCharacters(in: .whitespaces).isEmpty })
            Button("Show the phrase again") { self.asked = nil }
                .font(.system(.callout, weight: .semibold))
                .frame(maxWidth: .infinity, minHeight: 44)
        }
        .padding(16)
    }
}

/// Restore an identity from its 24 words.
struct RestoreSheet: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    @State private var text = ""
    @State private var error: String?

    private var words: [String] {
        text.split(whereSeparator: { $0.isWhitespace || $0 == "," }).map(String.init)
    }

    var body: some View {
        NavigationStack {
            VStack(alignment: .leading, spacing: 14) {
                Text("Type the 24 words of the identity to restore, separated by spaces. It replaces the identity on this phone.")
                    .font(.system(.subheadline))
                    .foregroundStyle(Theme.tx2)
                    .fixedSize(horizontal: false, vertical: true)
                TextEditor(text: $text)
                    .font(.system(.callout, design: .monospaced))
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
                    .scrollContentBackground(.hidden)
                    .padding(8)
                    .frame(minHeight: 160)
                    .background(Theme.s1, in: RoundedRectangle(cornerRadius: 12))
                    .overlay(RoundedRectangle(cornerRadius: 12).stroke(Theme.line))
                Text("\(words.count) / 24 words")
                    .font(.system(.footnote).monospacedDigit())
                    .foregroundStyle(words.count == 24 ? Theme.ok : Theme.tx2)
                if let error {
                    Banner(kind: .sos, title: error)
                }
                Spacer()
                Button("Restore") {
                    error = model.restore(words: words)
                    if error == nil { dismiss() }
                }
                .buttonStyle(.primary)
                .disabled(words.count != 24)
            }
            .padding(16)
            .screen()
            .secretCover()
            .navigationTitle("Restore an identity")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } }
            }
        }
    }
}

/// Our identity cannot be read or saved (the Keychain is locked, or holds something else):
/// nothing is replaced on its own. Retry, restore from the 24 words, or, when what is stored is
/// unreadable for good, start a new identity deliberately.
struct IdentityProblemView: View {
    @Environment(AppModel.self) private var model
    let problem: AppModel.IdentityProblem
    @State private var restoring = false
    @State private var restoringBackup = false
    @State private var confirmNew = false
    @State private var confirmEmpty = false
    @State private var error: String?

    /// The identity itself is the problem (not the database).
    private var identityAtFault: Bool {
        [.unavailable, .corrupt, .notSaved].contains(problem)
    }

    private var title: String {
        switch problem {
        case .storage: String(localized: "Your messages can’t be saved right now")
        case .databaseKeyUnavailable: String(localized: "Your messages can’t be opened right now")
        case .databaseKeyLost: String(localized: "Your messages can’t be opened")
        default: String(localized: "Your identity can’t be used right now")
        }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Spacer()
            Image(systemName: "person.badge.key")
                .font(.system(size: 40, weight: .regular))
                .foregroundStyle(Theme.sos)
            Text(title)
                .font(.system(.title2, weight: .bold))
                .foregroundStyle(Theme.tx)
            Text(explanation)
                .font(.system(.subheadline))
                .foregroundStyle(Theme.tx2)
                .fixedSize(horizontal: false, vertical: true)
            if let error {
                Banner(kind: .sos, title: error)
            }
            Spacer()
            Button("Try again") { model.retryIdentity() }
                .buttonStyle(.primary)
            if identityAtFault {
                Button("Restore from my 24 words") { restoring = true }
                    .buttonStyle(.secondary)
            }
            if identityAtFault || problem == .databaseKeyLost {
                Button("Restore a backup") { restoringBackup = true }
                    .buttonStyle(.secondary)
            }
            if problem == .databaseKeyLost {
                Button("Start with an empty history", role: .destructive) { confirmEmpty = true }
                    .frame(maxWidth: .infinity)
                    .padding(.top, 4)
            }
            if problem == .corrupt {
                Button("Start a new identity", role: .destructive) { confirmNew = true }
                    .frame(maxWidth: .infinity)
                    .padding(.top, 4)
            }
        }
        .padding(24)
        .screen()
        .sheet(isPresented: $restoring) { RestoreSheet() }
        .sheet(isPresented: $restoringBackup) { RestoreBackupSheet() }
        .confirmationDialog(
            "Start a new identity?", isPresented: $confirmNew, titleVisibility: .visible
        ) {
            Button("Start a new identity", role: .destructive) { error = model.startNewIdentity() }
        } message: {
            Text("Your contacts will see a new identity and will have to verify you again. Restore your 24 words instead if you have them.")
        }
        .confirmationDialog(
            "Start with an empty history?", isPresented: $confirmEmpty, titleVisibility: .visible
        ) {
            Button("Start with an empty history", role: .destructive) { error = model.discardUnreadableHistory() }
        } message: {
            Text("Your messages, contacts and groups on this phone are removed for good. Your identity stays: your contacts still know you. Restore a backup instead if you have one.")
        }
    }

    private var explanation: String {
        switch problem {
        case .unavailable:
            String(localized: "The phone’s secure storage can’t be read at the moment, often before the first unlock. Nothing was changed: unlock the phone, then try again.")
        case .corrupt:
            String(localized: "What is stored on this phone is not a valid identity. Restore it from its 24 words, or start a new identity.")
        case .notSaved:
            String(localized: "Your new identity could not be saved on this phone. Try again.")
        case .storage:
            String(localized: "The message database on this phone could not be opened, often because the phone is full. Nothing was sent or deleted: free some space, then try again.")
        case .databaseKeyUnavailable:
            String(localized: "Your messages are encrypted on this phone, and the key that opens them can’t be read at the moment, often before the first unlock. Nothing was changed: unlock the phone, then try again.")
        case .databaseKeyLost:
            String(localized: "Your messages are encrypted on this phone, and the key that opened them is gone: the phone’s secure storage was reset. Without it they can’t be read. Restore a backup, or start again with an empty history.")
        }
    }
}

/// Covers a screen holding the recovery phrase whenever the app is not active: the app
/// switcher's snapshot and screen sharing never show it (audit M13).
private struct SecretCover: ViewModifier {
    @Environment(\.scenePhase) private var phase

    func body(content: Content) -> some View {
        content.overlay {
            if phase != .active {
                ZStack {
                    Theme.bg
                    Image(systemName: "lock.fill").font(.system(size: 40)).foregroundStyle(Theme.tx2)
                }
                .ignoresSafeArea()
            }
        }
    }
}

extension View {
    func secretCover() -> some View { modifier(SecretCover()) }
}
