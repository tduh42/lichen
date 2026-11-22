import XCTest

/// Settings (docs/testing.md §9). ST-6 and ST-7 are the French and dark runs of the whole suite
/// (`--lang fr`, `--dark`); ST-9 is AccessibilityAuditTests.
final class SettingsTests: LichenUITest {
    /// A Settings row (a button whose label starts with `title`), scrolled to.
    private func settingsRow(_ title: String) -> XCUIElement {
        let row = app.buttons.matching(NSPredicate(format: "label BEGINSWITH %@", title)).firstMatch
        reveal(row)
        return row
    }

    /// A switch, scrolled to.
    private func settingsSwitch(_ title: String) -> XCUIElement {
        let toggle = app.switches[title]
        reveal(toggle)
        return toggle
    }

    /// Flips a switch (its knob, at its trailing end), and taps again if its value has not
    /// changed within 3 s: a slow runner drops a tap now and then (ST-8 on CI, run
    /// 45004362565: "Relay only" never came on, and the restart that followed found it off).
    private func flip(_ toggle: XCUIElement) {
        let before = toggle.value as? String
        for _ in 0..<3 {
            toggle.coordinate(withNormalizedOffset: CGVector(dx: 0.9, dy: 0.5)).tap()
            let deadline = Date().addingTimeInterval(3)
            while (toggle.value as? String) == before && Date() < deadline { pause(0.3) }
            if (toggle.value as? String) != before { return }
        }
    }

    /// ST-1: a privacy setting (GIFs loaded from GIPHY by themselves) applied, and kept after a
    /// restart; the app lock's own switch is ST-2.
    func testST1_PrivacySettingKept() {
        launch(["-tab", "settings"], demo: false)
        let gifs = settingsSwitch(tr("Load GIFs automatically"))
        let before = gifs.value as? String
        flip(gifs)
        XCTAssertNotEqual(gifs.value as? String, before, "the switch did not move")
        let after = gifs.value as? String
        relaunch(["-tab", "settings"], onboarded: true)
        XCTAssertEqual(settingsSwitch(tr("Load GIFs automatically")).value as? String, after, "not kept after a restart")
        // Back as it was, for the next tests (a fresh install resets it anyway).
        flip(settingsSwitch(tr("Load GIFs automatically")))
    }

    /// ST-2: the app lock on: the cover at launch, nothing of the app behind it. Unlocking needs
    /// Face ID matched from the Mac (`notifyutil`), outside the test.
    func testST2_AppLockCoversTheApp() {
        launch(["-lichen.lock", "YES", "-lockPrompt", "NO"])
        wait(button(tr("Unlock")), 10, "no lock cover")
        XCTAssertFalse(button(tr("New message")).isHittable, "Messages usable behind the cover")
        keep("st-lock")
    }

    /// ST-3: a backup made: its content counted, ready to save (the system's Files sheet). Its
    /// restore picks the file in that sheet: not run here (the core's backup tests cover it).
    func testST3_BackupExport() {
        launch(["-tab", "settings"], demo: false)
        settingsRow(tr("Export a backup")).tap()
        wait(containing(tr("Backup ready")), 20, "no backup")
        keep("st-backup")
        button(tr("Save to Files")).tap()
        // The system's sheet to save it. On a simulator just booted (CI) the first one can open
        // blank and close, the backup's sheet with it: the backup made again, saved again.
        let cancel = app.buttons.matching(NSPredicate(format: "label IN %@", ["Cancel", "Annuler"])).firstMatch
        if !cancel.waitForExistence(timeout: 15) && !button(tr("Save to Files")).exists {
            settingsRow(tr("Export a backup")).tap()
            wait(containing(tr("Backup ready")), 20, "no backup the second time")
            button(tr("Save to Files")).tap()
        }
        wait(cancel, 15, "no sheet to save it").tap()
    }

    /// ST-4: the problem report, built and shown before it is sent: versions and settings, and
    /// no names of the people the demo met.
    func testST4_ProblemReport() {
        launch(["-tab", "settings", "-quietDemo", "YES"])
        // Camille's first message, Marc's: names the report must not carry.
        tab(.messages)
        wait(row("Marc"), Self.camilleTimeout, "no message from Marc")
        tab(.settings)
        settingsRow(tr("Report a problem")).tap()
        let report = wait(app.descendants(matching: .any).matching(NSPredicate(format: "label BEGINSWITH 'lichen problem report'")).firstMatch,
                          10, "no report")
        let text = report.label
        XCTAssertTrue(text.contains("Versions:"), "no versions")
        for name in ["Camille", "Marc", "Julien", "Lucie", "refuge"] {
            XCTAssertFalse(text.contains(name), "\(name) in the report")
        }
        keep("st-report")
        button(tr("Share the report")).tap()
        let copy = app.descendants(matching: .any).matching(NSPredicate(format: "label IN %@", ["Copy", "Copier"])).firstMatch
        wait(copy, 10, "no share sheet")
        app.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.1)).tap()
    }

    /// ST-5: the version and build, as the release numbers them ("0.10.0 (1)").
    func testST5_Version() {
        launch(["-tab", "settings"], demo: false)
        reveal(element(tr("Version")))
        let number = app.staticTexts.matching(NSPredicate(format: "label MATCHES %@", #"\d+\.\d+\.\d+ \(\d+\)"#)).firstMatch
        XCTAssertTrue(number.exists, "no version and build next to Version")
    }

    /// ST-6 (B-ios-26, as Android's B-and-25): the radio's frequency and its sub-band in the
    /// figures of the language: "869,618 MHz" and "869,4–869,65 MHz" in French, with a point in
    /// English.
    func testST6_FiguresOfTheLanguage() {
        launch(["-tab", "settings"])
        let comma = french
        let frequency = comma ? "869,618 MHz" : "869.618 MHz"
        let band = comma ? "869,4–869,65 MHz" : "869.4–869.65 MHz"
        wait(containing(frequency), 30, "not \(frequency)")
        reveal(containing(band))
        XCTAssertTrue(containing(band).exists, "not \(band)")
        XCTAssertFalse(containing(comma ? "869.618" : "869,618").exists, "the other language's figures")
        keep("st-figures")
    }

    /// ST-8: the bridge's settings: iroh off and relay only on, kept after a restart; Tor on
    /// starts it, off stops it.
    func testST8_BridgeSettings() {
        launch(["-tab", "settings"], demo: false)
        let iroh = settingsSwitch(tr("iroh (direct P2P)"))
        let relay = settingsSwitch(tr("Relay only"))
        XCTAssertEqual(iroh.value as? String, "1", "iroh is not on by default")
        if relay.value as? String != "1" { flip(relay) }
        flip(iroh)
        waitValue(iroh, "0", 5)
        relaunch(["-tab", "settings"], onboarded: true)
        XCTAssertEqual(settingsSwitch(tr("iroh (direct P2P)")).value as? String, "0", "iroh back on after a restart")
        XCTAssertEqual(settingsSwitch(tr("Relay only")).value as? String, "1", "relay only off after a restart")
        // Tor on: the switch follows at once, and Tor starts (it may pass "Connecting" in an
        // instant on the way to publishing our onion service).
        let tor = settingsSwitch(tr("Tor"))
        flip(tor)
        waitValue(tor, "1", 2)
        let started = app.staticTexts.matching(NSPredicate(format: "label IN %@", [
            tr("Connecting to Tor…"), tr("Publishing your onion service…"),
            tr("Your onion service is reachable"), tr("Tor could not connect")])).firstMatch
        wait(started, 30, "Tor did not start")
        keep("st-tor")
        // Off: it stops.
        flip(tor)
        wait(element(tr("Anonymous, slower")), 10, "Tor did not stop")
        flip(settingsSwitch(tr("iroh (direct P2P)")))
    }

    /// ST-3: a backup made on a phone with a radio (FakeTcpRadio): a text to Marc, a group of
    /// its own, its radio's contacts; restored on a phone just installed, from its welcome
    /// screen, with its 24 words: all of it comes back. The Files sheets are stood in for
    /// (`-saveBackup`, `-restoreFile`), as Android answers its pickers.
    func testST3_BackupRestored() throws {
        let radio = try FakeTcpRadio()
        defer { radio.stop() }
        let file = "st3-test.lichen-backup"
        launch(["-tcp", radio.address, "-saveBackup", file], demo: false)
        wait(row("Marc"), 90, "no message from Marc").tap()
        send("Leaving at dawn")
        back()
        button(tr("New message")).tap()
        let name = app.textFields[tr("Group name")]
        for _ in 0..<8 where !(name.exists && name.isHittable) { app.collectionViews.firstMatch.swipeUp() }
        name.tap()
        name.typeText("Ridge walk")
        let create = button(tr("Create group"))
        if !create.isHittable { reveal(create); pause(1) }
        create.tap()
        wait(composer, 10, "the group did not open")
        back()
        // Its 24 words, then the backup.
        tab(.settings)
        settingsRow(tr("Your identity")).tap()
        cardButton(tr("Recovery phrase")).tap()
        wait(app.staticTexts["phrase.24"], 10, "no phrase")
        let words = (1...24).map { app.staticTexts["phrase.\($0)"].label }
        button(tr("Close")).tap()
        back()
        settingsRow(tr("Export a backup")).tap()
        wait(containing(tr("Backup ready")), 20, "no backup")
        button(tr("Save to Files")).tap()
        wait(containing(tr("Backup saved")), 5, "not saved")
        button(tr("Done")).tap()
        // Another phone: the app just installed, on the same radio.
        relaunch(["-tcp", radio.address, "-freshInstall", "YES", "-restoreFile", file], onboarded: false)
        button(tr("Restore a backup")).tap()
        app.buttons.matching(NSPredicate(format: "label BEGINSWITH %@", tr("Choose the backup file"))).firstMatch.tap()
        wait(app.buttons.matching(NSPredicate(format: "label BEGINSWITH %@", file)).firstMatch, 5, "no backup chosen")
        let editor = app.textViews.firstMatch
        editor.tap()
        editor.typeText(words.joined(separator: " "))
        wait(containing(tr("%lld / 24 words", 24)), 5, "not 24 words")
        keep("st-restore")
        button(tr("Restore")).tap()
        // All of it: our text to Marc, the group, the radio's contacts.
        let marc = wait(row("Marc"), 60, "Marc's chat did not come back")
        wait(row("Ridge walk"), 10, "the group did not come back")
        keep("st-restored")
        // Our text, in his chat: his row shows his reply to it once that has come (on a slow
        // machine, before the backup).
        marc.tap()
        wait(message("Leaving at dawn"), 10, "not our text")
        back()
        button(tr("New message")).tap()
        wait(app.buttons.matching(NSPredicate(format: "label BEGINSWITH 'Camille'")).firstMatch, 30, "the radio's contacts did not come back")
    }
}
