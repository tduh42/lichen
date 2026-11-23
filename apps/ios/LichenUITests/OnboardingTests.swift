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

    /// "I wrote them down", then the three words asked (D-7): `wrong` types a wrong first word.
    private func confirmPhrase(_ words: [String], wrong: Bool = false, file: StaticString = #filePath, line: UInt = #line) {
        wait(app.textFields["confirm.3"], 5, "no words asked", file: file, line: line)
        for k in 1...3 {
            let field = app.textFields["confirm.\(k)"]
            // "Word 5": its prompt.
            let prompt = [field.placeholderValue ?? "", field.label].first { !$0.isEmpty } ?? ""
            let position = Int(prompt.components(separatedBy: CharacterSet.decimalDigits.inverted).joined()) ?? 0
            XCTAssertTrue((1...24).contains(position), "asked: \(prompt)", file: file, line: line)
            field.tap()
            field.typeText(wrong && k == 1 ? "lichenzz" : words[position - 1])
        }
        button(tr("Check")).tap()
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
        // The phrase, written down and three of its words given back: no longer marked.
        cardButton(tr("Recovery phrase")).tap()
        confirmPhrase(phrase(close: "I wrote them down"))
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
        // No radio was ever paired: not one that dropped (D-1).
        XCTAssertTrue(containing(tr("No radio connected yet")).exists, "no radio prompt")
        XCTAssertTrue(containing(tr("Connect your radio to send and receive.")).exists)
        XCTAssertFalse(containing(tr("Radio disconnected")).exists, "a first launch says a radio dropped")
        keep("on-messages-empty")
        button(tr("Connect")).tap()
        wait(app.navigationBars[tr("Pair a radio")], 5, "Connect does not open the pairing")
    }

    /// ON-3, D-7: the phrase confirmed by three of its words; a wrong one is said, the phrase
    /// can be read again, and only three right words mark it backed up.
    func testON3_ConfirmTheWrittenPhrase() {
        launch(["-freshInstall", "YES", "-tab", "settings"], demo: false)
        let identity = cardButton(tr("Your identity"))
        reveal(identity)
        identity.tap()
        cardButton(tr("Recovery phrase")).tap()
        let words = phrase(close: "I wrote them down")
        confirmPhrase(words, wrong: true)
        wait(containing(tr("One of these words is not the one in your phrase.")), 5, "a wrong word is not said")
        keep("on-phrase-wrong")
        // Read again, then three right words.
        button(tr("Show the phrase again")).tap()
        confirmPhrase(phrase(close: "I wrote them down"))
        waitGone(element(tr("Not backed up")), 5, "still marked as not backed up")
        XCTAssertTrue(app.navigationBars[tr("Your identity")].exists, "the sheet stayed")
    }

    /// ON-3, D-7: closing the check without the three words leaves the phrase not backed up.
    func testON3_ClosingTheCheckIsNotABackup() {
        launch(["-freshInstall", "YES", "-tab", "settings"], demo: false)
        let identity = cardButton(tr("Your identity"))
        reveal(identity)
        identity.tap()
        cardButton(tr("Recovery phrase")).tap()
        _ = phrase(close: "I wrote them down")
        wait(app.textFields["confirm.1"], 5, "no words asked")
        button(tr("Close")).tap()
        wait(element(tr("Not backed up")), 5, "closing the check backed the phrase up")
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

    /// ON-4, D-3: our own code, pasted back, is said to be ours (not "could not be read").
    func testON4_OwnCodeIsSaidSo() throws {
        launch()
        wait(element(tr("%@ connected", "ThinkNode M5")), 20, "no radio")
        button(tr("Add a contact")).tap()
        let qr = wait(element(tr("Your contact code")), 10, "no code")
        let card = try XCTUnwrap(Self.decodeQR(qr.screenshot().image), "the code does not decode")
        UIPasteboard.general.string = card
        app.buttons[tr("Scan")].tap()
        let field = wait(app.textFields["pasteCode"], 5, "no paste field")
        field.tap()
        field.press(forDuration: 1.2)
        wait(app.menuItems.matching(NSPredicate(format: "label IN %@", ["Paste", "Coller"])).firstMatch, 5, "no Paste").tap()
        button(tr("Add")).tap()
        wait(containing(tr("This is your own code.")), 5, "our own code is not said to be ours")
        keep("on-own-code")
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

    /// The banner asking to compare safety numbers with `name` (B-user-1).
    private func compareBanner(_ name: String) -> XCUIElement {
        containing(tr("Compare safety numbers with %@", name))
    }

    /// ON-6, B-user-1, the phone that did not scan: Camille's key, learned from her first
    /// message and not verified, makes her chat ask to compare; Compare shows the numbers, and
    /// once they match the banner goes.
    func testON6_TheOtherPhoneIsAskedToCompare() {
        launch(["-open", "Camille", "-quietDemo", "YES"])
        wait(keyButton, Self.camilleTimeout, "no session with Camille")
        wait(compareBanner("Camille"), 5, "her chat does not ask to compare")
        keep("on-compare-banner")
        button(tr("Compare")).tap()
        let number = wait(app.descendants(matching: .any)["safetyNumber"], 5, "Compare shows no numbers")
        XCTAssertEqual(Self.groups(number.label, length: 5).count, 12, number.label)
        button(tr("The numbers match")).tap()
        waitGone(number, 5)
        waitGone(compareBanner("Camille"), 5, "the banner stayed once verified")
        waitLabel(keyButton, contains: tr("Key verified"), 5)
    }

    /// ON-6, B-user-1: Later puts the banner away, after a restart too; neither compared nor
    /// put off, it comes back after a restart.
    func testON6_CompareLaterAndAfterARestart() {
        launch(["-open", "Camille", "-quietDemo", "YES"])
        wait(keyButton, Self.camilleTimeout, "no session with Camille")
        wait(compareBanner("Camille"), 5, "her chat does not ask to compare")
        relaunch(["-open", "Camille", "-quietDemo", "YES"], demo: true)
        wait(keyButton, Self.camilleTimeout, "no session with Camille after a restart")
        wait(compareBanner("Camille"), 5, "the banner did not come back after a restart")
        button(tr("Later")).tap()
        waitGone(compareBanner("Camille"), 5, "Later did not put it away")
        relaunch(["-open", "Camille", "-quietDemo", "YES"], demo: true)
        wait(keyButton, Self.camilleTimeout, "no session with Camille after a restart")
        pause(2)
        XCTAssertFalse(compareBanner("Camille").exists, "put off, it came back after a restart")
        XCTAssertEqual(keyButton.label, tr("Verify their key"), "Later verified her")
    }

    /// ON-6, D-10: at an accessibility text size the banner is compact: its title, Compare and
    /// Later, without the note; still above the messages.
    func testON6_CompactBannerAtLargeText() {
        launch(["-open", "Camille", "-quietDemo", "YES",
                "-UIPreferredContentSizeCategoryName", "UICTContentSizeCategoryAccessibilityL"])
        wait(keyButton, Self.camilleTimeout, "no session with Camille")
        wait(compareBanner("Camille"), 5, "her chat does not ask to compare")
        XCTAssertTrue(button(tr("Compare")).exists && button(tr("Later")).exists, "not both buttons")
        XCTAssertFalse(containing(tr("Their key is not verified yet. Compare the numbers with them, in person or on a call.")).exists,
                       "the note at an accessibility text size")
        keep("on-compare-banner-large")
    }

    /// ON-6, B-ios-29: with the keyboard up the banner is compact too, in every language: its
    /// title, Compare and Later, without the note, so that the messages keep most of the room
    /// above the keyboard (in French the full banner left them about 100 pt). The keyboard
    /// down, the note comes back.
    func testON6_CompactBannerWithTheKeyboardUp() {
        launch(["-open", "Camille", "-quietDemo", "YES"])
        wait(keyButton, Self.camilleTimeout, "no session with Camille")
        let banner = wait(compareBanner("Camille"), 5, "her chat does not ask to compare")
        let note = containing(tr("Their key is not verified yet. Compare the numbers with them, in person or on a call."))
        XCTAssertTrue(note.exists, "no note, the keyboard down")
        send("On my way up")
        XCTAssertTrue(app.keyboards.firstMatch.exists, "no keyboard after a text")
        waitGone(note, 5, "the note stayed with the keyboard up")
        let last = wait(message("On my way up"), 5)
        XCTAssertTrue(last.isHittable, "the last message is not on screen")
        // The messages: from below the banner to the composer, more room than the banner's.
        let room = composer.frame.minY - banner.frame.maxY
        XCTAssertGreaterThan(room, banner.frame.height, "the banner (\(Int(banner.frame.height)) pt) leaves the messages \(Int(room)) pt")
        keep("on-compare-banner-keyboard")
        // A drag from the messages down into the keyboard puts it down (as in Messages): the
        // note comes back.
        let list = app.scrollViews.containing(NSPredicate(format: "label BEGINSWITH %@ OR label BEGINSWITH %@",
                                                          "On my way up", "Hi! I'm at the refuge")).firstMatch
        list.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.3))
            .press(forDuration: 0.05, thenDragTo: app.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.97)))
        waitGone(app.keyboards.firstMatch, 5, "the keyboard stayed up")
        wait(note, 5, "the note did not come back with the keyboard down")
    }

    /// ON-7, pasted: Léa's code added by paste is not verified; her conversation starts and a
    /// text reaches her.
    func testON7_AddAContactByPastingTheirCode() {
        launch(["-screen", "contact", "-contactDemo", "paste", "-quietDemo", "YES"])
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
        send("Hello Léa")
        waitText(tr("Delivered"), 60)
    }

    /// ON-7, scanned in person (the camera's stand-in, `-contactDemo scan`): Léa added,
    /// verified; then our code for her to scan (B-user-1); her conversation starts and asks
    /// nothing more.
    func testON7_AddAContactByScanningTheirCode() throws {
        launch(["-contactDemo", "scan", "-quietDemo", "YES"])
        let added = wait(containing(tr("%@ added, verified", "Léa")), 20, "Léa not added by the scan")
        XCTAssertLessThan(added.frame.height, 100, "the banner is stretched: \(added.frame)")
        keep("on-scan-added")
        button(tr("Done")).tap()
        wait(containing(tr("Now let %@ scan your code", "Léa")), 5, "our code is not offered for her to scan")
        let qr = wait(element(tr("Your contact code")), 5, "no code to scan")
        let card = try XCTUnwrap(Self.decodeQR(qr.screenshot().image), "the code does not decode")
        XCTAssertTrue(card.hasPrefix("lichen:1:"), card)
        keep("on-show-your-code")
        button(tr("Done")).tap()
        wait(composer, 10, "her conversation did not open")
        wait(keyButton, 5)
        XCTAssertEqual(keyButton.label, tr("Key verified"))
        XCTAssertFalse(compareBanner("Léa").exists, "a verified contact is asked to compare")
        send("Hello Léa")
        waitText(tr("Delivered"), 60)
    }
}
