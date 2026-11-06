import SwiftUI

/// "Your identity" (mockup `identite`, ADR-0013): name, key fingerprint, backup of the
/// 24-word phrase, restore.
struct IdentityView: View {
    /// In the first-launch flow: what "Continue" does.
    var next: (() -> Void)?
    @Environment(AppModel.self) private var model
    @State private var showingPhrase = false
    @State private var restoring = false

    var body: some View {
        @Bindable var model = model
        ScrollView {
            VStack(spacing: 16) {
                Avatar(name: model.myName.isEmpty ? model.nodeName : model.myName, key: model.core.key() ?? Data(), size: 76)
                    .padding(.top, 8)
                TextField("Your name", text: $model.myName)
                    .font(.system(size: 18, weight: .medium))
                    .multilineTextAlignment(.center)
                    .padding(.vertical, 12)
                    .frame(maxWidth: 240)
                    .background(Theme.s1, in: RoundedRectangle(cornerRadius: 12))
                    .overlay(RoundedRectangle(cornerRadius: 12).stroke(Theme.line))
                    .submitLabel(.done)

                VStack(alignment: .leading, spacing: 12) {
                    Label("Your key's fingerprint", systemImage: "key")
                        .font(.system(size: 16, weight: .semibold))
                        .labelStyle(TintedIconLabel(tint: Theme.ok))
                    KeyFingerprint(groups: model.fingerprint)
                        .frame(maxWidth: .infinity)
                    Text("An X25519 / Ed25519 key made on this phone. It never leaves it: your radio only carries the messages.")
                        .font(.system(size: 13))
                        .foregroundStyle(Theme.tx2)
                        .fixedSize(horizontal: false, vertical: true)
                }
                .padding(16)
                .background(Theme.s1, in: RoundedRectangle(cornerRadius: 14))

                if !model.phraseBackedUp {
                    Banner(kind: .warn, title: String(localized: "Not backed up"),
                           message: String(localized: "Without a backup, losing this phone means losing your identity and your verified contacts."))
                }

                CardList {
                    Button { showingPhrase = true } label: {
                        CardRow(title: String(localized: "Recovery phrase"),
                                subtitle: String(localized: "24 words to write on paper")) {
                            Image(systemName: "chevron.right").foregroundStyle(Theme.tx2)
                        }
                    }
                    .buttonStyle(.plain)
                    CardDivider()
                    Button { restoring = true } label: {
                        CardRow(title: String(localized: "Restore an identity"),
                                subtitle: String(localized: "From its 24 words: replaces this one")) {
                            Image(systemName: "chevron.right").foregroundStyle(Theme.tx2)
                        }
                    }
                    .buttonStyle(.plain)
                    CardDivider()
                    CardRow(title: String(localized: "Encrypted backup"),
                            subtitle: String(localized: "A password-protected file: coming later"))
                        .opacity(0.5)
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
    }
}

/// The 24 words, to write on paper.
struct RecoveryPhraseSheet: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        let words = model.recoveryPhrase()
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 16) {
                    Text("Write these 24 words on paper, in order, and keep them somewhere safe. Anyone who has them can be you.")
                        .font(.system(size: 15))
                        .foregroundStyle(Theme.tx2)
                        .fixedSize(horizontal: false, vertical: true)
                    LazyVGrid(columns: [GridItem(.flexible()), GridItem(.flexible())], alignment: .leading, spacing: 10) {
                        ForEach(Array(words.enumerated()), id: \.offset) { i, w in
                            HStack(spacing: 8) {
                                Text("\(i + 1)")
                                    .font(.system(size: 13).monospacedDigit())
                                    .foregroundStyle(Theme.tx2)
                                    .frame(width: 22, alignment: .trailing)
                                Text(verbatim: w).font(.system(size: 17, weight: .semibold, design: .monospaced))
                            }
                            .padding(.vertical, 6)
                            .padding(.horizontal, 10)
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .background(Theme.s1, in: RoundedRectangle(cornerRadius: 10))
                        }
                    }
                    .privacySensitive()
                    Button("I wrote them down") {
                        model.phraseBackedUp = true
                        dismiss()
                    }
                    .buttonStyle(.primary)
                    .padding(.top, 8)
                }
                .padding(16)
            }
            .screen()
            .navigationTitle("Recovery phrase")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Close") { dismiss() } }
            }
        }
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
                    .font(.system(size: 14))
                    .foregroundStyle(Theme.tx2)
                    .fixedSize(horizontal: false, vertical: true)
                TextEditor(text: $text)
                    .font(.system(size: 16, design: .monospaced))
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
                    .scrollContentBackground(.hidden)
                    .padding(8)
                    .frame(minHeight: 160)
                    .background(Theme.s1, in: RoundedRectangle(cornerRadius: 12))
                    .overlay(RoundedRectangle(cornerRadius: 12).stroke(Theme.line))
                Text("\(words.count) / 24 words")
                    .font(.system(size: 13).monospacedDigit())
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
            .navigationTitle("Restore an identity")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } }
            }
        }
    }
}
