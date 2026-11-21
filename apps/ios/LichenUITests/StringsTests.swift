import XCTest

/// The app's French words (its String Catalog, compiled into this bundle): no English left
/// where the French UI has its own word (D-6); the typographic apostrophe in both languages
/// (B-ios-26). No app is launched.
final class StringsTests: XCTestCase {
    /// Every French string and plural form, by key.
    private func french() throws -> [String: String] {
        let all = try table("fr")
        XCTAssertGreaterThan(all.count, 500, "the French strings are not all there")
        return all
    }

    /// Every string and plural form of `lang` compiled into this bundle, by key ("key [one]" for
    /// a plural form). English has its plural forms only: its other text is the keys.
    private func table(_ lang: String) throws -> [String: String] {
        let bundle = Bundle(for: StringsTests.self)
        let dir = try XCTUnwrap(bundle.path(forResource: lang, ofType: "lproj"), "no \(lang) in the bundle")
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
        return all
    }

    /// `key` rendered with `args`, from the String Catalog compiled into this bundle, plural
    /// forms included.
    private func render(_ key: String, _ args: [CVarArg], _ lang: String) throws -> String {
        let dir = try XCTUnwrap(Bundle(for: StringsTests.self).path(forResource: lang, ofType: "lproj"), "no \(lang)")
        let bundle = try XCTUnwrap(Bundle(path: dir))
        let format = bundle.localizedString(forKey: key, value: key, table: "Localizable")
        return String(format: format, locale: Locale(identifier: lang == "fr" ? "fr_FR" : "en_GB"), arguments: args)
    }

    /// B-ios-23 (after B-ios-17): one of a thing is said in the singular, in English and French.
    func testB23_CountsOfOneInTheSingular() throws {
        let cases: [(String, [CVarArg], String, String)] = [
            ("Queued %@ for %u conversations", ["now", UInt32(1)], "Queued now for 1 conversation", "En file now pour 1 conversation"),
            ("%lld measures", [1], "1 measure", "1 mesure"),
            ("%lld measures · range calibrated on their GPS", [1],
             "1 measure · range calibrated on their GPS", "1 mesure · portée calibrée sur son GPS"),
            ("%lld messages · %lld media", [1, 1], "1 message · 1 media", "1 message · 1 média"),
            ("%lld messages, %lld media files, %@", [1, 1, "2 KB"], "1 message, 1 media file, 2 KB",
             "1 message, 1 fichier média, 2 KB"),
            ("%lld direct messages and %lld channel posts imported.", [1, 1],
             "1 direct message and 1 channel post imported.", "1 message direct et 1 message de canal importés."),
            ("%lld were already here.", [1], "1 was already here.", "1 y était déjà."),
            ("%lld bytes too long: MeshCore takes %lld", [1, 150], "1 byte too long: MeshCore takes 150",
             "1 octet de trop : MeshCore en prend 150"),
            ("%lld members · flood", [1], "1 member · flood", "1 membre · flood"),
        ]
        for (key, args, en, fr) in cases {
            XCTAssertEqual(try render(key, args, "en"), en, key)
            XCTAssertEqual(try render(key, args, "fr"), fr, key)
        }
        // And two, in the plural.
        XCTAssertEqual(try render("%lld measures", [2], "en"), "2 measures")
        XCTAssertEqual(try render("%lld messages, %lld media files, %@", [2, 3, "2 KB"], "fr"), "2 messages, 3 fichiers médias, 2 KB")
    }

    /// B-ios-26 (as Android's B-and-16): the typographic apostrophe ’ in every English and French
    /// text, never a straight one. English is the keys and its plural forms.
    func testB26_TypographicApostrophe() throws {
        let fr = try french()
        XCTAssertEqual(fr.filter { $0.value.contains("'") }.keys.sorted(), [], "French with a straight apostrophe")
        let english = Array(fr.keys) + Array(try table("en").values)
        XCTAssertEqual(english.filter { $0.contains("'") }.sorted(), [], "English with a straight apostrophe")
        XCTAssertEqual(fr["The radio’s GPS"], "Le GPS de la radio")
        XCTAssertEqual(fr["Airtime left this hour"], "Temps d’antenne restant cette heure")
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
