import XCTest

/// The app's French words (its String Catalog, compiled into this bundle): no English left
/// where the French UI has its own word (D-6). No app is launched.
final class StringsTests: XCTestCase {
    /// Every French string and plural form, by key.
    private func french() throws -> [String: String] {
        let bundle = Bundle(for: StringsTests.self)
        let dir = try XCTUnwrap(bundle.path(forResource: "fr", ofType: "lproj"), "no French in the bundle")
        var all: [String: String] = [:]
        let strings = URL(fileURLWithPath: dir).appendingPathComponent("Localizable.strings")
        if let table = NSDictionary(contentsOf: strings) as? [String: String] {
            all.merge(table) { a, _ in a }
        }
        // Plurals: each form's text.
        let plurals = URL(fileURLWithPath: dir).appendingPathComponent("Localizable.stringsdict")
        if let table = NSDictionary(contentsOf: plurals) as? [String: [String: Any]] {
            for (key, entry) in table {
                for case let (name, rule as [String: Any]) in entry where name != "NSStringLocalizedFormatKey" {
                    for (form, text) in rule where form != "NSStringFormatSpecTypeKey" && form != "NSStringFormatValueTypeKey" {
                        if let text = text as? String { all["\(key) [\(form)]"] = text }
                    }
                }
            }
        }
        XCTAssertGreaterThan(all.count, 500, "the French strings are not all there")
        return all
    }

    /// D-6: « Envoyer une annonce », « Préréglage », « Clés d’envoi … »; no "advert", "Preset" or
    /// "Sender keys" left in French.
    func testD6_NoEnglishWordsLeftInFrench() throws {
        let fr = try french()
        XCTAssertEqual(fr["Send an advert"], "Envoyer une annonce")
        XCTAssertEqual(fr["Preset"], "Préréglage")
        XCTAssertEqual(fr["Waiting for sender keys"], "Clés d’envoi en attente")
        XCTAssertEqual(fr["Sender keys up to date"], "Clés d’envoi à jour")
        let english = ["advert", "Preset", "Sender key", "sender key"]
        let left = fr.filter { _, text in english.contains { text.contains($0) } }
        XCTAssertTrue(left.isEmpty, "English left in French: \(left)")
    }
}
