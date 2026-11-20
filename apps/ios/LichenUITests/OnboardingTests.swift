import CryptoKit
import XCTest

/// Onboarding and identity (docs/testing.md §1). A first launch is made with `-freshInstall
/// YES` (the identity, the database key, the data and the settings removed first).
final class OnboardingTests: LichenUITest {
    /// The fingerprint on the identity screen, as its 6 groups.
    private func fingerprint(file: StaticString = #filePath, line: UInt = #line) -> [String] {
        let f = wait(app.descendants(matching: .any)["fingerprint"], 10, "no fingerprint", file: file, line: line)
        return Self.groups(f.label, length: 4)
    }

    /// The groups of `length` hexadecimal digits or digits in `text`.
    static func groups(_ text: String, length: Int) -> [String] {
        let regex = try! NSRegularExpression(pattern: "\\b[0-9A-F]{\(length)}\\b")
        return regex.matches(in: text, range: NSRange(text.startIndex..., in: text))
            .compactMap { Range($0.range, in: text).map { String(text[$0]) } }
    }

    /// The 24 words of the recovery phrase sheet, then closes it with `close`.
    private func phrase(close: String, file: StaticString = #filePath, line: UInt = #line) -> [String] {
        wait(app.staticTexts["phrase.24"], 10, "no phrase", file: file, line: line)
        let words = (1...24).map { app.staticTexts["phrase.\($0)"].label }
        XCTAssertEqual(Set(words).count > 12, true, "words: \(words)", file: file, line: line)
        button(tr(close)).tap()
        return words
    }

    /// The welcome screens of a first launch, "Later" at each step: the identity screen keeps
    /// its fingerprint. Returns it.
    private func onboardWithoutRadio() -> [String] {
        launch(["-freshInstall", "YES"], demo: false, onboarded: false)
        wait(element(tr("Encrypted messaging off the grid, over LoRa and beyond.")), 10, "no welcome")
        keep("on-welcome")
        button(tr("Later")).tap()
        wait(app.navigationBars[tr("Your identity")], 5, "no identity screen")
        let groups = fingerprint()
        XCTAssertEqual(groups.count, 6, "fingerprint: \(groups)")
        XCTAssertTrue(element(tr("Not backed up")).exists, "a new identity is not marked as not backed up")
        keep("on-identity")
        // The phrase, written down: no longer marked.
        cardButton(tr("Recovery phrase")).tap()
        _ = phrase(close: "I wrote them down")
        waitGone(element(tr("Not backed up")), 5, "still marked as not backed up")
        button(tr("Continue")).tap()
        wait(app.navigationBars[tr("Add a contact")], 5, "no contact screen")
        // No radio: no code to show yet; said in a banner its own height.
        let warning = wait(containing(tr("Connect your radio first")), 5, "no radio warning on My code")
        XCTAssertLessThan(warning.frame.height, 150, "the banner is stretched: \(warning.frame)")
        keep("on-contact")
        button(tr("Later")).tap()
        answerNotifications(timeout: 2)
        return groups
    }

    /// ON-1: the welcome screens to the end: an identity made, its phrase shown and written
    /// down, then Messages, empty, asking for the radio.
    func testON1_FirstLaunchToTheEnd() {
        _ = onboardWithoutRadio()
        wait(element(tr("No conversations yet")), 10, "Messages is not empty")
        XCTAssertTrue(element(tr("Radio disconnected")).exists, "no radio prompt")
        XCTAssertTrue(button(tr("Retry")).exists, "no way to retry the radio")
        keep("on-messages-empty")
    }

    /// ON-2: started again, the same identity and no welcome screens.
    func testON2_RestartKeepsTheIdentity() {
        let before = onboardWithoutRadio()
        wait(element(tr("No conversations yet")), 10)
        relaunch()
        wait(element(tr("No conversations yet")), 10, "not on Messages after a restart")
        XCTAssertFalse(element(tr("Encrypted messaging off the grid, over LoRa and beyond.")).exists, "welcome again")
        tab(.settings)
        let identity = cardButton(tr("Your identity"))
        reveal(identity)
        identity.tap()
        XCTAssertEqual(fingerprint(), before, "another identity after the restart")
    }

    /// ON-3: the recovery phrase is the identity's: restored from it, the same fingerprint;
    /// a phrase with a wrong word, or 23 words, is refused.
    func testON3_RecoveryPhraseRestoresTheSameIdentity() {
        launch(["-freshInstall", "YES"], demo: false)
        tab(.settings)
        let identity = cardButton(tr("Your identity"))
        reveal(identity)
        identity.tap()
        let before = fingerprint()
        cardButton(tr("Recovery phrase")).tap()
        let words = phrase(close: "Close")
        // 23 words: Restore stays off.
        cardButton(tr("Restore an identity")).tap()
        let editor = wait(app.textViews.firstMatch, 5, "no text field")
        editor.tap()
        editor.typeText(words.dropLast().joined(separator: " "))
        let restore = button(tr("Restore"))
        XCTAssertFalse(restore.isEnabled, "Restore with 23 words")
        // A word that is not in the list: refused, nothing replaced.
        editor.typeText(" lichenzz")
        XCTAssertTrue(restore.isEnabled)
        restore.tap()
        pause(1)
        XCTAssertTrue(app.navigationBars[tr("Restore an identity")].exists, "a wrong phrase was accepted")
        keep("on-restore-refused")
        button(tr("Cancel")).tap()
        XCTAssertEqual(fingerprint(), before, "a refused phrase changed the identity")
        // The right words: the same identity.
        cardButton(tr("Restore an identity")).tap()
        let again = wait(app.textViews.firstMatch, 5)
        again.tap()
        again.typeText(words.joined(separator: " "))
        button(tr("Restore")).tap()
        waitGone(app.textViews.firstMatch, 10, "the phrase was refused")
        XCTAssertEqual(fingerprint(), before, "restored from its phrase, another identity")
    }

    /// ON-3, in the demo: its identity is a throwaway one; writing its words down must not mark
    /// the phone's own identity as backed up.
    func testON3_TheDemoDoesNotBackUpTheRealIdentity() {
        launch(["-freshInstall", "YES", "-tab", "settings"])
        let identity = cardButton(tr("Your identity"))
        reveal(identity)
        identity.tap()
        wait(app.descendants(matching: .any)["fingerprint"], 10)
        if cardButton(tr("Recovery phrase")).exists {
            cardButton(tr("Recovery phrase")).tap()
            _ = phrase(close: "I wrote them down")
        }
        keep("on-demo-identity")
        back()
        let leave = cardButton(tr("Leave the demo"))
        reveal(leave)
        leave.tap()
        let mine = cardButton(tr("Your identity"))
        reveal(mine)
        mine.tap()
        wait(app.descendants(matching: .any)["fingerprint"], 10)
        XCTAssertTrue(element(tr("Not backed up")).exists, "the phone's identity is marked as backed up by the demo's words")
    }

    /// ON-4: my code holds my identity and my name: the QR decodes to a card whose key has the
    /// fingerprint shown, and whose name is the one I gave.
    func testON4_MyCodeDecodesToMyCard() throws {
        launch(["-tab", "settings"])
        // A name of my own.
        let identity = cardButton(tr("Your identity"))
        reveal(identity)
        identity.tap()
        let name = wait(app.textFields.firstMatch, 5, "no name field")
        name.tap()
        name.typeText("Alex\n")
        let groups = fingerprint()
        keep("on-identity-named")
        back()
        tab(.messages)
        // Its code, once the radio is connected (the card names the radio's node).
        wait(element(tr("%@ connected", "ThinkNode M5")), 20, "no radio")
        button(tr("Add a contact")).tap()
        let qr = wait(element(tr("Your contact code")), 10, "no code")
        XCTAssertTrue(element("Alex").exists, "the code does not say whose it is")
        keep("on-my-code")
        let card = try XCTUnwrap(Self.decodeQR(qr.screenshot().image), "the code does not decode")
        XCTAssertTrue(card.hasPrefix("lichen:1:"), card)
        let raw = try XCTUnwrap(Self.base64url(String(card.dropFirst("lichen:1:".count))), "not a card: \(card)")
        XCTAssertGreaterThan(raw.count, 64)
        let key = raw.prefix(32)
        XCTAssertEqual(String(decoding: raw.dropFirst(64), as: UTF8.self), "Alex", "the card's name")
        var hash = SHA256()
        hash.update(data: Data("lichen/1 fingerprint".utf8))
        hash.update(data: key)
        let digest = Array(hash.finalize())
        let expected = (0..<6).map { String(format: "%02X%02X", digest[2 * $0], digest[2 * $0 + 1]) }
        XCTAssertEqual(expected, groups, "the card's key is not the identity shown")
    }

    static func decodeQR(_ image: UIImage) -> String? {
        guard let ci = CIImage(image: image),
              let detector = CIDetector(ofType: CIDetectorTypeQRCode, context: nil, options: [CIDetectorAccuracy: CIDetectorAccuracyHigh])
        else { return nil }
        return detector.features(in: ci).compactMap { ($0 as? CIQRCodeFeature)?.messageString }.first
    }

    static func base64url(_ text: String) -> Data? {
        var s = text.replacingOccurrences(of: "-", with: "+").replacingOccurrences(of: "_", with: "/")
        while s.count % 4 != 0 { s += "=" }
        return Data(base64Encoded: s)
    }

    /// ON-5: each identity or database failure shows its screen, its explanation and its ways
    /// out; Try again leaves it once the cause is gone.
    func testON5_IdentityFailureScreens() {
        let cases: [(String, String, [String])] = [
            ("unavailable", "Your identity can’t be used right now", ["Try again", "Restore from my 24 words", "Restore a backup"]),
            ("corrupt", "Your identity can’t be used right now", ["Try again", "Restore from my 24 words", "Restore a backup", "Start a new identity"]),
            ("notSaved", "Your identity can’t be used right now", ["Try again", "Restore from my 24 words", "Restore a backup"]),
            ("storage", "Your messages can’t be saved right now", ["Try again"]),
            ("databaseKey", "Your messages can’t be opened right now", ["Try again"]),
            ("databaseKeyLost", "Your messages can’t be opened", ["Try again", "Restore a backup", "Start with an empty history"]),
        ]
        let all = ["Try again", "Restore from my 24 words", "Restore a backup", "Start a new identity", "Start with an empty history"]
        for (problem, title, ways) in cases {
            launch(["-identityProblem", problem], demo: false)
            wait(element(tr(title)), 10, "\(problem): no “\(title)”")
            for way in all {
                XCTAssertEqual(button(tr(way)).exists, ways.contains(way), "\(problem): “\(way)”")
            }
            keep("on-problem-\(problem)")
            // The identity is fine on this phone: Try again leaves the screen.
            button(tr("Try again")).tap()
            waitGone(element(tr(title)), 10, "\(problem): Try again did not leave the screen")
            wait(app.tabBars.firstMatch, 5, "\(problem): no app after Try again")
            app.terminate()
        }
    }

    /// ON-6: Camille's key, verified: her safety number (12 groups of 5 digits), "The numbers
    /// match", then the lock says so; "They differ" takes it back.
    func testON6_VerifyAContact() {
        launch(["-open", "Camille"])
        wait(keyButton, Self.camilleTimeout, "no session with Camille")
        XCTAssertEqual(keyButton.label, tr("Verify their key"))
        keyButton.tap()
        let number = wait(app.descendants(matching: .any)["safetyNumber"], 5, "no safety number")
        XCTAssertEqual(Self.groups(number.label, length: 5).count, 12, number.label)
        keep("on-verify")
        button(tr("The numbers match")).tap()
        waitGone(number, 5, "the sheet stayed")
        waitLabel(keyButton, contains: tr("Key verified"), 5)
        waitText(tr("Encrypted session · key verified"), 5)
        // Verified: the numbers are a check, and "They differ" takes it back.
        keyButton.tap()
        wait(element(tr("Verified by the scan. Their phone shows the same numbers once it knows you: after your first message, or when they scan your code.")), 5)
        button(tr("They differ")).tap()
        waitLabel(keyButton, contains: tr("Verify their key"), 5)
    }

    /// Settings › Send an advert, then back to the chat: Léa's radio must know ours before it
    /// answers our path discovery (bugs-ios.md B-ios-1).
    private func advertise() {
        tab(.settings)
        let advert = wait(cardButton(tr("Send an advert")), 5)
        advert.tap()
        wait(containing(tr("Sent: radios around can add yours as a contact.")), 5)
        tab(.messages)
        wait(composer, 5)
    }

    /// ON-7, pasted: Léa's code added by paste is not verified; her conversation starts and a
    /// text reaches her.
    func testON7_AddAContactByPastingTheirCode() {
        launch(["-screen", "contact", "-contactDemo", "paste"])
        wait(app.navigationBars[tr("Add a contact")], 10, "no contact screen")
        app.buttons[tr("Scan")].tap()
        let field = wait(app.textFields["pasteCode"], 5, "no paste field")
        // The card reaches the pasteboard once the radio is up.
        pause(4)
        field.tap()
        field.press(forDuration: 1.2)
        let paste = app.menuItems.matching(NSPredicate(format: "label IN %@", ["Paste", "Coller"])).firstMatch
        wait(paste, 5, "no Paste").tap()
        XCTAssertTrue((field.value as? String ?? "").hasPrefix("lichen:1:"), "pasted: \(field.value ?? "")")
        button(tr("Add")).tap()
        wait(element(tr("Compare these numbers with %@", "Léa")), 5, "no safety number for Léa")
        XCTAssertFalse(containing(tr("%@ added, verified", "Léa")).exists, "a pasted code verified Léa")
        keep("on-paste-added")
        button(tr("The numbers match")).tap()
        wait(composer, 10, "her conversation did not open")
        advertise()
        send("Hello Léa")
        waitText(tr("Delivered"), 60)
    }

    /// ON-7, scanned in person (the camera's stand-in, `-contactDemo scan`): Léa added,
    /// verified; her conversation starts.
    func testON7_AddAContactByScanningTheirCode() {
        launch(["-contactDemo", "scan"])
        let added = wait(containing(tr("%@ added, verified", "Léa")), 20, "Léa not added by the scan")
        XCTAssertLessThan(added.frame.height, 100, "the banner is stretched: \(added.frame)")
        keep("on-scan-added")
        button(tr("Done")).tap()
        wait(composer, 10, "her conversation did not open")
        wait(keyButton, 5)
        XCTAssertEqual(keyButton.label, tr("Key verified"))
        advertise()
        send("Hello Léa")
        waitText(tr("Delivered"), 60)
    }
}
