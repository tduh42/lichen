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

    /// Flips a switch (its knob, at its trailing end).
    private func flip(_ toggle: XCUIElement) {
        toggle.coordinate(withNormalizedOffset: CGVector(dx: 0.9, dy: 0.5)).tap()
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
        // The system's sheet to save it.
        let cancel = app.buttons.matching(NSPredicate(format: "label IN %@", ["Cancel", "Annuler"])).firstMatch
        wait(cancel, 10, "no sheet to save it").tap()
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
}
