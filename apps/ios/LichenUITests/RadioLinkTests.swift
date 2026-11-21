import XCTest

/// The radio link and the radio's settings (docs/testing.md §2), on the demo network's
/// simulated companion ("ThinkNode M5"), which drops and comes back on cue (`-offline`,
/// `-online`) and lets texts expire sooner (`-expiry`).
final class RadioLinkTests: LichenUITest {
    private var connected: String { tr("%@ connected", "ThinkNode M5") }

    /// RL-1: the demo radio connects; its contacts (people only) and its channel arrive.
    func testRL1_DemoRadioConnects() {
        launch()
        wait(element(connected), 10, "the radio did not connect")
        wait(row("Public"), 10, "no Public channel")
        // The radio's card, a tap away: airtime and LoRa available.
        app.descendants(matching: .any)["radioLine"].tap()
        wait(containing(tr("Airtime left this hour")), 5, "no radio card")
        XCTAssertTrue(element(tr("available")).exists, "LoRa not available")
        // The simulated radio is not on Bluetooth.
        XCTAssertFalse(containing("BLE ·").exists, "the demo's radio card says BLE")
        XCTAssertTrue(containing("SF8").exists, "no LoRa parameters on the card")
        // The radio's name whole: its parameters under it, not beside it.
        let headline = app.staticTexts.matching(NSPredicate(format: "label == %@", connected)).firstMatch
        let parameters = app.staticTexts.containing(NSPredicate(format: "label CONTAINS 'SF8'")).firstMatch
        XCTAssertGreaterThanOrEqual(parameters.frame.minY, headline.frame.maxY - 1,
                                    "the parameters share the name's line and cut it: \(headline.frame) \(parameters.frame)")
        keep("rl-radio-card")
        button(tr("New message")).tap()
        for name in ["Camille", "Julien", "Marc"] {
            wait(app.buttons.containing(NSPredicate(format: "label BEGINSWITH %@", name)).firstMatch, 10, "no contact \(name)")
        }
        // Repeaters and rooms are nodes, not people.
        for node in ["hub", "crest", "Refuge board"] {
            XCTAssertFalse(app.buttons.containing(NSPredicate(format: "label BEGINSWITH %@", node)).firstMatch.exists,
                           "\(node) listed with the people")
        }
        XCTAssertTrue(app.buttons.containing(NSPredicate(format: "label BEGINSWITH 'Public'")).firstMatch.exists, "no channel")
        keep("rl-new-message")
    }

    /// RL-2: the radio drops (Messages says so), a text written meanwhile waits, and goes
    /// once, when the radio is back.
    func testRL2_RadioDropsAndComesBack() {
        launch(["-offline", "22", "-online", "45", "-quietDemo", "YES"])
        openChat("Camille")
        let written = tr("Written while my radio is off")
        let text = waitText(written, 40)
        waitLabel(text, contains: tr("Waiting"), 5)
        XCTAssertTrue(containing(tr("Radio not connected: messages wait for it, up to 24 h")).exists, "the composer does not say it waits")
        keep("rl-waiting")
        back()
        wait(element(tr("No radio connected")), 5, "the radio line does not say it dropped")
        XCTAssertTrue(element(tr("Radio disconnected")).exists, "no Radio disconnected banner")
        keep("rl-disconnected")
        wait(element(connected), 40, "the radio did not come back")
        XCTAssertFalse(element(tr("Radio disconnected")).exists, "the banner stayed")
        openChat("Camille")
        waitLabel(message(written), contains: tr("Delivered"), 60)
        // Camille got it: her answer quotes it. Ours shows once.
        waitText("Copy: \"\(written)\"", 30)
        XCTAssertEqual(bubbles(written), 1, "the waiting text shows more than once")
    }

    /// RL-3: a text that waits past its expiry is marked failed, and can be sent again.
    func testRL3_TextExpiresWhileOffline() {
        launch(["-offline", "22", "-expiry", "20", "-quietDemo", "YES"])
        openChat("Camille")
        let written = tr("Written while my radio is off")
        waitText(written, 40)
        wait(element(tr("Expired")), 30, "the text did not expire")
        waitLabel(message(written), contains: tr("Not delivered"), 5)
        keep("rl-expired")
        // The radio back (Settings › Demo), then Resend.
        tab(.settings)
        let radio = app.switches[tr("Simulated radio connected")]
        reveal(radio)
        radio.tap()
        if (radio.value as? String) == "0" { radio.coordinate(withNormalizedOffset: CGVector(dx: 0.9, dy: 0.5)).tap() }
        tab(.messages)
        waitGone(containing(tr("Radio not connected: messages wait for it, up to 24 h")), 10, "the radio is not back")
        button(tr("Resend")).tap()
        keep("rl-resent")
        waitLabel(message(written), contains: tr("Delivered"), 60)
        XCTAssertFalse(element(tr("Expired")).exists, "still marked expired")
    }

    /// RL-4: the radios this phone used are listed; one forgotten leaves the list, also after a
    /// restart; the other can be connected again (here nothing answers: the screen says so).
    func testRL4_SavedRadios() {
        launch(["-freshInstall", "YES", "-savedRadios", "YES", "-tab", "settings"], demo: false)
        wait(cardButton(tr("Radios")), 10)
        XCTAssertTrue(cardButton(tr("Radios")).label.contains(tr("%lld saved", 2)), "not 2 saved radios: \(cardButton(tr("Radios")).label)")
        cardButton(tr("Radios")).tap()
        wait(element("Heltec V4"), 5, "no Heltec V4")
        XCTAssertTrue(element("Workshop bridge").exists, "no Workshop bridge")
        XCTAssertTrue(containing(tr("TCP")).exists && containing(tr("Bluetooth")).exists, "links not shown")
        keep("rl-radios")
        button(tr("Actions for %@", "Heltec V4")).tap()
        wait(button(tr("Forget")), 5).tap()
        // Confirmed.
        let confirm = app.sheets.buttons[tr("Forget")].exists ? app.sheets.buttons[tr("Forget")] : app.buttons.matching(NSPredicate(format: "label == %@", tr("Forget"))).element(boundBy: 0)
        wait(confirm, 5, "no confirmation").tap()
        waitGone(element("Heltec V4"), 5, "Heltec V4 still listed")
        XCTAssertTrue(element("Workshop bridge").exists)
        relaunch(["-tab", "settings"], onboarded: true)
        wait(cardButton(tr("Radios")), 10)
        XCTAssertTrue(cardButton(tr("Radios")).label.contains(tr("%lld saved", 1)), "after a restart: \(cardButton(tr("Radios")).label)")
        // A radio was paired before: it dropped, rather than "no radio yet" (D-1).
        tab(.messages)
        wait(containing(tr("Radio disconnected")), 5, "a radio paired before is not said to have dropped")
        XCTAssertFalse(containing(tr("No radio connected yet")).exists)
        tab(.settings)
        cardButton(tr("Radios")).tap()
        wait(element("Workshop bridge"), 5)
        XCTAssertFalse(element("Heltec V4").exists)
        // Connect: it is tried; nothing answers there, and the screen says so.
        button(tr("Actions for %@", "Workshop bridge")).tap()
        wait(button(tr("Connect")), 5, "no Connect for a radio not connected").tap()
        wait(containing(tr("Could not reach the radio: %@", "#").components(separatedBy: "#")[0]), 15,
             "a radio that does not answer: nothing said")
        keep("rl-reconnect")
    }

    // MARK: The radio's settings (ADR-0032)

    private func openRadioSettings() {
        launch(["-tab", "settings", "-screen", "radio"])
        wait(app.staticTexts.containing(NSPredicate(format: "label BEGINSWITH '869.618 MHz'")).firstMatch, 30,
             "the radio's parameters are not shown")
    }

    /// Flips a switch whose label is hidden (the row shows its title).
    private func flip(_ toggle: XCUIElement) {
        let before = toggle.value as? String
        toggle.tap()
        if toggle.value as? String == before {
            toggle.coordinate(withNormalizedOffset: CGVector(dx: 0.9, dy: 0.5)).tap()
        }
    }

    /// RL-5: what the radio reports, and changes read back from it: the GPS, a position in
    /// adverts (asked first), the transmit power, a preset.
    func testRL5_RadioSettingsReadChangeReadBack() {
        openRadioSettings()
        XCTAssertTrue(element("869.618 MHz · 62.5 kHz · SF8 · CR 4/8").exists || containing("869.618 MHz").exists)
        keep("rl-settings")
        // The transmit power, one step down, as the radio reports it.
        let power = app.staticTexts.matching(NSPredicate(format: "label ENDSWITH ' dBm'")).firstMatch
        wait(power, 5, "no transmit power")
        let dbm = Int(power.label.split(separator: " ").first ?? "") ?? 0
        app.steppers.firstMatch.buttons.element(boundBy: 0).tap()
        wait(element(tr("%lld dBm", dbm - 1)), 10, "the power did not come back from the radio")
        // The demo's board has a GPS, off.
        let gps = app.switches[tr("The radio's GPS")]
        reveal(gps)
        XCTAssertEqual(gps.value as? String, "0")
        flip(gps)
        wait(element(tr("A fix every")), 10, "the GPS did not come back on")
        // A position in adverts asks first.
        let position = app.switches[tr("Position in adverts")]
        reveal(position)
        flip(position)
        let confirm = app.alerts[tr("Show your position to the whole mesh?")]
        wait(confirm, 5, "no confirmation")
        confirm.buttons[tr("Show it")].tap()
        wait(element(tr("Every advert shows the radio's position to the whole mesh, unencrypted.")), 10,
             "the radio did not report the new policy")
        // How far adverts are added from: "Up to 1 hop", in the singular.
        let far = app.buttons.matching(NSPredicate(format: "label CONTAINS %@ OR label CONTAINS %@",
                                                   tr("Any distance"), tr("Direct only"))).firstMatch
        reveal(far)
        far.tap()
        wait(button(tr("Up to %lld hops", 1)), 5, "no Up to 1 hop")
        XCTAssertFalse(button(tr("Up to %lld hops", 2).replacingOccurrences(of: "2", with: "1")).exists, "Up to 1 hops")
        button(tr("Up to %lld hops", 1)).tap()
        wait(app.buttons.matching(NSPredicate(format: "label CONTAINS %@", tr("Up to %lld hops", 1))).firstMatch, 10,
             "the hops did not come back from the radio")
        // A preset: USA/Canada, then back.
        reveal(element(tr("Preset")), up: false)
        setPreset(tr("USA/Canada"))
        wait(app.staticTexts.containing(NSPredicate(format: "label BEGINSWITH '910.525 MHz'")).firstMatch, 10,
             "the new preset did not come back from the radio")
        setPreset(tr("EU/UK (narrow)"))
        wait(app.staticTexts.containing(NSPredicate(format: "label BEGINSWITH '869.618 MHz'")).firstMatch, 10)
        reveal(element(tr("Restart the radio")))
    }

    /// Picks a preset in the LoRa card (not applied yet).
    private func choosePreset(_ name: String) {
        let picker = app.buttons.matching(NSPredicate(format: "label BEGINSWITH %@ OR label CONTAINS %@", tr("Preset"), tr("Custom"))).firstMatch
        let current = [tr("EU/UK (narrow)"), tr("USA/Canada"), tr("Custom")].map { button($0) }.first { $0.exists && $0.isHittable }
        (current ?? picker).tap()
        wait(button(name), 5, "no preset \(name)").tap()
    }

    /// Picks a preset and applies it, confirmed.
    private func setPreset(_ name: String) {
        choosePreset(name)
        button(tr("Apply")).tap()
        let alert = app.alerts[tr("Change the LoRa settings?")]
        wait(alert, 5, "no confirmation")
        alert.buttons[tr("Apply")].tap()
    }

    /// RL-6: a value refused (a frequency out of range, a PIN the radio does not take): said,
    /// and the old value stays.
    func testRL6_RefusedSetting() {
        openRadioSettings()
        choosePreset(tr("Custom"))
        // Custom with the same values: nothing to apply. A frequency of 100 MHz is refused.
        let frequency = app.textFields["869.618"].exists ? app.textFields["869.618"] : app.textFields.firstMatch
        reveal(frequency, up: true)
        frequency.tap()
        frequency.press(forDuration: 1.0)
        if app.menuItems.matching(NSPredicate(format: "label IN %@", ["Select All", "Tout sélectionner"])).firstMatch.exists {
            app.menuItems.matching(NSPredicate(format: "label IN %@", ["Select All", "Tout sélectionner"])).firstMatch.tap()
        }
        frequency.typeText(String(repeating: XCUIKeyboardKey.delete.rawValue, count: 10) + "100")
        button(tr("Apply")).tap()
        let alert = app.alerts[tr("Change the LoRa settings?")]
        wait(alert, 5)
        alert.buttons[tr("Apply")].tap()
        let refused = app.alerts[tr("Not changed")]
        wait(refused, 5, "a frequency of 100 MHz was not refused")
        keep("rl-refused")
        refused.buttons["OK"].tap()
        XCTAssertTrue(app.staticTexts.containing(NSPredicate(format: "label BEGINSWITH '869.618 MHz'")).firstMatch.exists,
                      "the old frequency is gone")
        // A PIN with a leading zero is not six digits for the radio.
        let pin = app.textFields["123456"]
        reveal(pin)
        pin.tap()
        pin.typeText("012345")
        button(tr("Save the PIN")).tap()
        wait(app.alerts[tr("Not changed")], 5, "the PIN 012345 was not refused")
        app.alerts[tr("Not changed")].buttons["OK"].tap()
        XCTAssertFalse(containing(tr("Saved: used from the radio's next restart. Pair the phone again with it.")).exists,
                       "a refused PIN is said to be saved")
    }

    /// RL-5, D-9: a radio slow to answer (`-radioSlow 40`, 10 s at speed 4): its settings are
    /// said to be read, not shown as zeros or as no radio, until it answers; then its values.
    func testRL5_SettingsBeingRead() {
        launch(["-tab", "settings", "-screen", "radio", "-radioSlow", "40"])
        wait(containing(tr("Reading the radio's settings…")), 10, "not said to be reading")
        XCTAssertFalse(containing("0.000 MHz").exists, "zeros shown as the radio's values")
        XCTAssertFalse(containing(tr("Connect a radio to see its settings.")).exists, "a connected radio said to be missing")
        keep("rl-reading")
        let power = app.staticTexts.matching(NSPredicate(format: "label ENDSWITH ' dBm'")).firstMatch
        wait(power, 60, "the radio's values did not come")
        XCTAssertTrue(containing("869.618 MHz").exists, "not the radio's frequency")
        XCTAssertFalse(containing(tr("Reading the radio's settings…")).exists, "still said to be reading")
    }

    /// RL-6, D-4: a setting the radio itself refuses (`-radioRefuses power`): said, and the
    /// power shown stays the radio's.
    func testRL6_TheRadioRefusesASetting() {
        launch(["-tab", "settings", "-screen", "radio", "-radioRefuses", "power"])
        let power = app.staticTexts.matching(NSPredicate(format: "label ENDSWITH ' dBm'")).firstMatch
        wait(power, 30, "no transmit power")
        let before = power.label
        app.steppers.firstMatch.buttons.element(boundBy: 0).tap()
        let refused = app.alerts[tr("Not changed")]
        wait(refused, 10, "the radio's refusal is not said")
        XCTAssertTrue(refused.staticTexts[tr("The radio did not take this setting")].exists, "the refusal's words")
        keep("rl-radio-refused")
        refused.buttons["OK"].tap()
        XCTAssertEqual(power.label, before, "the power shown is not the radio's")
    }

    /// D-8: with `-demoSpeed 4` the demo runs four times faster: Camille's first message, about
    /// 18 s into the demo, comes in under 10 s.
    func testRL1_TheDemoRunsFasterWithDemoSpeed() {
        let start = Date()
        launch(["-quietDemo", "YES"], speed: 4)
        wait(row("Camille"), 30, "no message from Camille")
        let took = Date().timeIntervalSince(start)
        XCTAssertLessThan(took, 10, "Camille's first message after \(Int(took)) s")
    }

    /// F-1: the radio's MeshCore public key (64 hex digits, in groups of 8), copied whole; its
    /// MeshCore code is the stock app's contact code for this radio, with that key.
    func testF1_TheRadiosMeshCoreKeyAndCode() throws {
        openRadioSettings()
        let hex = checkMeshCoreKey(meshcoreKey)
        XCTAssertTrue(cardButton(tr("Share the key")).exists, "no way to share the key")
        let qr = element(tr("MeshCore code of this radio"))
        reveal(qr)
        let code = try XCTUnwrap(OnboardingTests.decodeQR(qr.screenshot().image), "the code does not decode")
        XCTAssertTrue(code.hasPrefix("meshcore://contact/add?"), code)
        XCTAssertTrue(code.contains("public_key=\(hex)"), "not the key shown: \(code)")
        XCTAssertTrue(code.contains("type=1"), code)
        XCTAssertTrue(containing(tr("Someone with the stock MeshCore app can scan it to add this radio.")).exists, "not said what it is for")
        keep("rl-meshcore-code")
        // Copied whole: the 64 digits, without the spaces shown between groups.
        XCTAssertEqual(pasted(), hex, "not the key copied")
    }

    /// RL-7: the radio's battery in its settings; with Show radio details, the radio card on
    /// Messages and the fragments and airtime under each message.
    func testRL7_BatteryAndRadioDetails() {
        openRadioSettings()
        XCTAssertTrue(containing(tr("Battery")).exists, "no battery")
        XCTAssertTrue(containing("4.01 V").exists, "the radio's battery is not shown")
        app.terminate()
        launch(["-lichen.radioDetails", "YES"])
        wait(containing(tr("Airtime left this hour")), 10, "no radio card on Messages")
        XCTAssertFalse(app.descendants(matching: .any)["radioLine"].exists, "the line instead of the card")
        keep("rl-details-card")
        openChat("Camille")
        wait(keyButton, Self.camilleTimeout)
        send("Radio details")
        // "Delivered · 1 fragment · 0.4 s of airtime".
        waitLabel(message("Radio details"), contains: tr("Delivered") + " · 1 fragment", 60)
        keep("rl-details-chat")
    }
}
