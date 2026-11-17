import SwiftUI

/// Settings › About › Report a problem (ADR-0045): the whole report on screen, then the share
/// sheet. No message, key, name or position is in it.
struct ReportView: View {
    @Environment(AppModel.self) private var model
    @State private var report = ""
    @State private var file: URL?

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 12) {
                Text("What lichen did lately, for its developers: versions, your radio's model, firmware and settings, and the recent log of the app. No message, key, name or position is in it. Read it, then send it with a few words on what went wrong.")
                    .font(.system(.subheadline))
                    .foregroundStyle(Theme.tx2)
                    .fixedSize(horizontal: false, vertical: true)
                if let file {
                    ShareLink(item: file) {
                        Label("Share the report", systemImage: "square.and.arrow.up")
                    }
                    .buttonStyle(.primary)
                }
                Text(verbatim: report)
                    .font(.system(.caption2, design: .monospaced))
                    .textSelection(.enabled)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .card(padding: 12)
            }
            .padding(16)
        }
        .screen()
        .navigationTitle("Report a problem")
        .navigationBarTitleDisplayMode(.inline)
        .toolbarBackground(Theme.s1, for: .navigationBar)
        .toolbarBackground(.visible, for: .navigationBar)
        .onAppear { refresh() }
    }

    private func refresh() {
        report = model.problemReport()
        let url = FileManager.default.temporaryDirectory.appendingPathComponent("lichen-report.txt")
        file = (try? report.write(to: url, atomically: true, encoding: .utf8)) == nil ? nil : url
    }
}

/// What `assets/licences/licences.json` holds (made by `scripts/licences.py`): lichen, what it
/// includes, the libraries of each app, and the Rust crates of the core with their licence files.
struct Licences: Decodable, Sendable {
    struct Entry: Decodable, Identifiable, Hashable, Sendable {
        let name: String
        let version: String
        let licence: String
        let url: String
        let platforms: [String]?
        let texts: [Int]
        var id: String { "\(name) \(version) \(platforms?.joined(separator: ",") ?? "")" }
    }

    let components: [Entry]
    let crates: [Entry]
    let texts: [String]

    static func load() -> Licences? {
        guard let url = Bundle.main.url(forResource: "licences", withExtension: "json"),
              let data = try? Data(contentsOf: url) else { return nil }
        return try? JSONDecoder().decode(Licences.self, from: data)
    }
}

/// Settings › About › Licences.
struct LicencesView: View {
    @State private var licences: Licences?

    var body: some View {
        ScrollView {
            LazyVStack(alignment: .leading, spacing: 8) {
                Text("lichen is free software under the GNU General Public License, version 3 or later: you may use it, study it, share it and change it. Its source code is on GitHub.")
                    .font(.system(.subheadline))
                    .foregroundStyle(Theme.tx2)
                    .fixedSize(horizontal: false, vertical: true)
                    .padding(.horizontal, 4)
                if let licences {
                    SectionLabel("Included").padding(.top, 10)
                    rows(licences.components.filter { $0.platforms?.contains("ios") ?? true }, in: licences)
                    SectionLabel("Rust crates of the core (\(licences.crates.count))").padding(.top, 10)
                    rows(licences.crates, in: licences)
                } else {
                    ProgressView().frame(maxWidth: .infinity).padding(.top, 40)
                }
            }
            .padding(16)
        }
        .screen()
        .navigationTitle("Licences")
        .navigationBarTitleDisplayMode(.inline)
        .toolbarBackground(Theme.s1, for: .navigationBar)
        .toolbarBackground(.visible, for: .navigationBar)
        .task {
            if licences == nil { licences = await Task.detached { Licences.load() }.value }
        }
    }

    private func rows(_ entries: [Licences.Entry], in all: Licences) -> some View {
        CardList {
            ForEach(Array(entries.enumerated()), id: \.element.id) { i, e in
                if i > 0 { CardDivider() }
                NavigationLink {
                    LicenceView(entry: e, texts: e.texts.compactMap { all.texts.indices.contains($0) ? all.texts[$0] : nil })
                } label: {
                    CardRow(title: e.version.isEmpty ? e.name : "\(e.name) \(e.version)", subtitle: e.licence) {
                        Image(systemName: "chevron.right").foregroundStyle(Theme.tx2)
                    }
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
            }
        }
    }
}

/// One component or crate: its licence, its link, and the licence files it ships.
struct LicenceView: View {
    let entry: Licences.Entry
    let texts: [String]

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 12) {
                Text(verbatim: entry.licence).font(.system(.body, weight: .semibold))
                if let url = URL(string: entry.url) {
                    Link(destination: url) {
                        Text(verbatim: entry.url).font(.system(.footnote)).multilineTextAlignment(.leading)
                    }
                }
                if texts.isEmpty {
                    Text("The licence is on the page above.")
                        .font(.system(.footnote))
                        .foregroundStyle(Theme.tx2)
                }
                ForEach(Array(texts.enumerated()), id: \.offset) { _, text in
                    Text(verbatim: text)
                        .font(.system(.caption2, design: .monospaced))
                        .textSelection(.enabled)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .card(padding: 12)
                }
            }
            .padding(16)
        }
        .screen()
        .navigationTitle(entry.name)
        .navigationBarTitleDisplayMode(.inline)
        .toolbarBackground(Theme.s1, for: .navigationBar)
        .toolbarBackground(.visible, for: .navigationBar)
    }
}
