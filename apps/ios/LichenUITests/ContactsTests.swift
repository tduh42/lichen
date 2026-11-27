import XCTest

/// Contacts, nodes, room servers, repeaters, introduction requests and blocks (docs/testing.md
/// §3), on the demo network: the radio knows Camille and Julien (lichen), Marc (MeshCore), the
/// repeaters hub and crest and the "Refuge board" room server; it hears Lucie's advert.
final class ContactsTests: LichenUITest {
    /// The New message sheet's contact whose label starts with `name` (its line says lichen or
    /// MeshCore chat; Messages' rows, behind the sheet, do not).
    private func contact(_ name: String) -> XCUIElement {
        app.buttons.matching(NSPredicate(format: "label BEGINSWITH %@ AND (label CONTAINS %@ OR label CONTAINS %@)",
                                         name, ", " + tr("lichen") + ",", tr("MeshCore chat · not lichen-encrypted"))).firstMatch
    }

    /// The Nodes tab, then the node `name`.
    private func openNode(_ name: String, file: StaticString = #filePath, line: UInt = #line) {
        tab(.nodes)
        let row = app.buttons.matching(NSPredicate(format: "label BEGINSWITH %@", name)).firstMatch
        wait(row, 20, "no node \(name)", file: file, line: line).tap()
        wait(app.navigationBars[name], 5, "\(name) did not open", file: file, line: line)
    }

    /// CT-1: the contacts are people only, lichen users first then by name, each with their
    /// hops.
    func testCT1_ContactsList() {
        launch()
        // Once Camille's identity is known (her first message).
        wait(row("Camille"), Self.camilleTimeout, "no Camille")
        button(tr("New message")).tap()
        let camille = wait(contact("Camille"), 10)
        let julien = wait(contact("Julien"), 10)
        let marc = wait(contact("Marc"), 10)
        XCTAssertTrue(camille.label.contains(tr("lichen")), camille.label)
        XCTAssertTrue(camille.label.contains(tr("LoRa · %lld hops", 1)), camille.label)
        XCTAssertTrue(julien.label.contains(tr("LoRa · %lld hops", 2)), julien.label)
        XCTAssertTrue(marc.label.contains(tr("MeshCore chat · not lichen-encrypted")), marc.label)
        XCTAssertTrue(marc.label.contains(tr("LoRa · %lld hops", 1)), marc.label)
        // Lichen users first, then by name.
        XCTAssertLessThan(camille.frame.minY, marc.frame.minY)
        if julien.label.contains(tr("lichen")) { XCTAssertLessThan(julien.frame.minY, marc.frame.minY) }
        for node in ["hub", "crest", "Refuge board", "Lucie"] {
            XCTAssertFalse(contact(node).exists, "\(node) listed with the contacts")
        }
        keep("ct-contacts")
    }

    /// CT-2: Camille renamed on this phone: the chat's title, Messages and the notification
    /// use the new name, the node's own name stays under it; reset, her own name again.
    func testCT2_RenameAContact() {
        // At the wall clock's pace: her answer comes once Messages is on screen, not while her
        // chat still is (no banner then).
        launch(["-open", "Camille", "-quietDemo", "YES"], speed: 1, banners: true)
        wait(keyButton, Self.camilleTimeout, "no session with Camille")
        let title = app.navigationBars.buttons.matching(NSPredicate(format: "label BEGINSWITH 'Camille'")).firstMatch
        wait(title, 5, "no title button").tap()
        let alert = app.alerts[tr("Rename")]
        wait(alert, 5, "no rename")
        alert.textFields.firstMatch.typeText("Cam")
        alert.buttons[tr("Save")].tap()
        let renamed = app.navigationBars.buttons.matching(NSPredicate(format: "label BEGINSWITH 'Cam,' OR label == 'Cam'")).firstMatch
        wait(renamed.exists ? renamed : containing("Cam"), 5)
        XCTAssertTrue(app.navigationBars.firstMatch.descendants(matching: .any)
            .matching(NSPredicate(format: "label CONTAINS 'Camille ·'")).firstMatch.exists, "the node's name is not shown under")
        keep("ct-renamed")
        // A message from her while Messages is on screen: the row and the banner say "Cam".
        send("What time is dinner?")
        back()
        wait(row("Cam"), 5, "the row is not renamed")
        XCTAssertFalse(row("Camille").exists, "the row still says Camille")
        let banner = XCUIApplication(bundleIdentifier: "com.apple.springboard").descendants(matching: .any)
            .matching(NSPredicate(format: "label CONTAINS 'Copy: \"What time is dinner'")).firstMatch
        XCTAssertTrue(banner.waitForExistence(timeout: 40), "no notification of her answer")
        XCTAssertTrue(banner.label.contains("Cam") && !banner.label.contains("Camille"), "the notification: \(banner.label)")
        // Her own name again.
        row("Cam").tap()
        wait(composer, 5)
        app.navigationBars.buttons.matching(NSPredicate(format: "label BEGINSWITH 'Cam'")).firstMatch.tap()
        wait(alert, 5)
        alert.buttons[tr("Use %@ again", "Camille")].tap()
        wait(app.navigationBars.buttons.matching(NSPredicate(format: "label BEGINSWITH 'Camille'")).firstMatch, 5,
             "her own name did not come back")
    }

    /// CT-3: the Nodes tab opens on the node list, under its large title; Settings no longer
    /// lists the nodes.
    func testCT3_NodesTabOpensOnTheList() {
        launch(["-quietDemo", "YES"])
        tab(.nodes)
        wait(app.navigationBars[tr("Nodes")], 5, "the Nodes tab has no title")
        wait(app.buttons.matching(NSPredicate(format: "label BEGINSWITH 'hub'")).firstMatch, 20, "no hub in the list")
        keep("ct-nodes-tab")
        tab(.settings)
        wait(cardButton(tr("Radio settings")), 10, "not on Settings")
        let nodes = NSPredicate(format: "label BEGINSWITH %@", tr("Nodes"))
        XCTAssertEqual(app.buttons.matching(nodes).count, app.tabBars.buttons.matching(nodes).count,
                       "Settings still lists the nodes")
    }

    /// A node's row in the Nodes tab.
    private func nodeRow(_ name: String) -> XCUIElement {
        app.buttons.matching(NSPredicate(format: "identifier == 'node' AND label BEGINSWITH %@", name)).firstMatch
    }

    /// The part of a localized format before its first argument: "Signal good, ". As many
    /// arguments as any of these formats takes (fewer would have it read past them).
    private func prefix(_ key: String) -> String {
        tr(key, "#", "#", "#", "#").components(separatedBy: "#")[0]
    }

    /// A filter chip ("Repeaters 2"), whatever its count.
    private func chip(_ key: String) -> XCUIElement {
        app.buttons.matching(NSPredicate(format: "label BEGINSWITH %@ AND NOT identifier == 'node'", tr(key))).firstMatch
    }

    /// CT-3: the nodes the radio knows or heard, each with its kind and when last heard. In
    /// reach (heard directly, ADR-0049), by rating, each with its meter: the hub Good, Lucie Fair,
    /// the board Bad (the demo's SNRs drift a little: the rating is checked, the figure read).
    /// Through repeaters: crest and Camille one hop away, in the singular. The filters.
    func testCT3_Nodes() {
        launch()
        tab(.nodes)
        let hub = nodeRow("hub"), lucie = nodeRow("Lucie"), board = nodeRow("Refuge board"), crest = nodeRow("crest")
        for (row, rating) in [(hub, "Signal good, %@"), (lucie, "Signal fair, %@"), (board, "Signal weak, %@")] {
            wait(row, 30, "no node")
            waitLabel(row, contains: prefix(rating), 30)
            XCTAssertTrue(row.label.contains(" dB"), row.label)
        }
        XCTAssertTrue(element(tr("In reach · heard directly by your radio")).exists, "no In reach")
        XCTAssertTrue(element(tr("Through repeaters")).exists, "no Through repeaters")
        // By rating, then the nodes reached through repeaters.
        XCTAssertLessThan(hub.frame.minY, lucie.frame.minY)
        XCTAssertLessThan(lucie.frame.minY, board.frame.minY)
        XCTAssertLessThan(board.frame.minY, crest.frame.minY)
        for (row, kind) in [(hub, "Repeater"), (crest, "Repeater"), (board, "Room server"), (lucie, "MeshCore app")] {
            XCTAssertTrue(row.label.contains(tr(kind)), row.label)
        }
        XCTAssertTrue(lucie.label.contains(tr("heard")), "Lucie is not marked as heard")
        XCTAssertTrue(crest.label.contains(tr("%lld hops", 1)) && crest.label.contains(tr("via %@", "hub")), crest.label)
        // One hop is said in the singular.
        let camille = nodeRow("Camille").label
        XCTAssertTrue(camille.contains(tr("%lld hops", 1)), camille)
        XCTAssertFalse(camille.contains(tr("%lld hops", 2).replacingOccurrences(of: "2", with: "1")), camille)
        keep("ct-nodes")
        // The filters, each with its count.
        chip("Repeaters").tap()
        XCTAssertFalse(board.exists)
        XCTAssertTrue(hub.exists)
        chip("Rooms").tap()
        XCTAssertTrue(board.exists)
        XCTAssertFalse(hub.exists)
        // Heard in 2 h: as many nodes as the header says were heard lately.
        chip("Heard in 2 h").tap()
        let summary = app.staticTexts.matching(NSPredicate(format: "label CONTAINS %@", " · ")).firstMatch
        let heard = summary.label.components(separatedBy: " · ").last?.split(separator: " ").first.flatMap { Int($0) }
        XCTAssertEqual(app.buttons.matching(identifier: "node").count, heard, summary.label)
        chip("All").tap()
        XCTAssertTrue(board.exists && hub.exists)
    }

    /// CT-3: a lichen user's row: once Camille's identity is known, the lichen badge, not
    /// "MeshCore app" (the nodes are read again when an identity is learned).
    func testCT3_LichenUserInTheNodes() {
        launch()
        wait(row("Camille"), Self.camilleTimeout, "no Camille")
        tab(.nodes)
        let camille = wait(nodeRow("Camille"), 10, "no node for Camille")
        XCTAssertTrue(camille.label.contains("lichen"), camille.label)
        XCTAssertFalse(camille.label.contains(tr("MeshCore app")), camille.label)
    }

    /// CT-3: the hub's page: its large meter, its signal over the last 24 h (a reading at
    /// least), now; a week, then a day again.
    func testCT3_HubPage() {
        launch()
        tab(.nodes)
        waitLabel(wait(nodeRow("hub"), 30, "no hub"), contains: prefix("Signal good, %@"), 30)
        openNode("hub")
        wait(app.descendants(matching: .any).matching(NSPredicate(format: "label BEGINSWITH %@", prefix("Signal good, %@"))).firstMatch,
             5, "no meter")
        wait(element(tr("Signal, last 24 h")), 5, "no signal chart")
        let chart = tr("Signal chart: %lld readings, now %@", 1, "#").components(separatedBy: ":")[0]
        XCTAssertTrue(containing(chart).exists, "no chart")
        XCTAssertTrue(containing(prefix("now %@")).exists, "no figure now")
        keep("ct-node-hub")
        button(tr("7 days")).tap()
        wait(element(tr("Signal, last 7 days")), 5, "not a week")
        button(tr("24 h")).tap()
        wait(element(tr("Signal, last 24 h")), 5, "not a day")
        XCTAssertFalse(containing(prefix("Heard only through repeaters: no signal of its own. The route above shows how it reaches you.")).exists)
    }

    /// CT-4: logged in to the board, its posts arrive in a row of its own, and a post goes out.
    func testCT4_RoomLoginReadAndPost() {
        launch(["-roomLogin", "hello"])
        let board = wait(row("Refuge board"), 60, "no row for the board")
        XCTAssertTrue(board.label.contains(tr("MeshCore room")), board.label)
        board.tap()
        wait(composer, 5)
        waitText("Hut keeper here", 10)
        waitText("Left a pair of gloves", 10)
        send("Arriving at 6, two of us")
        waitLabel(message("Arriving at 6"), contains: tr("Delivered"), 60)
        keep("ct-room")
    }

    /// CT-5: a wrong room password is refused, and the screen says so.
    func testCT5_RoomWrongPassword() {
        launch()
        openNode("Refuge board")
        let field = app.secureTextFields[tr("Room password")]
        reveal(field)
        field.tap()
        field.typeText("wrong")
        app.buttons["serverLogin"].tap()
        let said = app.staticTexts.matching(NSPredicate(format: "label IN %@",
            [tr("Refused"), tr("No answer: a wrong password, or out of reach")])).firstMatch
        wait(said, 30, "a wrong password is not said")
        XCTAssertFalse(containing(tr("Logged in")).exists, "logged in with a wrong password")
        keep("ct-room-refused")
    }

    /// CT-6: the hub's admin: logged in, its status read, a command answered.
    func testCT6_RepeaterAdmin() {
        launch()
        openNode("hub")
        let field = app.secureTextFields[tr("Admin or guest password")]
        reveal(field)
        field.tap()
        field.typeText("password")
        app.buttons["serverLogin"].tap()
        wait(element(tr("Logged in as admin")), 30, "not logged in as admin")
        let status = button(tr("Read the status"))
        reveal(status)
        status.tap()
        wait(containing(tr("Up for")), 30, "no status")
        XCTAssertTrue(containing(tr("Packets")).exists)
        keep("ct-repeater-status")
        let console = cardButton(tr("Console"))
        reveal(console)
        console.tap()
        let input = wait(app.textFields.firstMatch, 5, "no command line")
        input.tap()
        input.typeText("ver")
        button(tr("Send")).tap()
        wait(app.staticTexts.matching(NSPredicate(format: "label CONTAINS[c] 'v1.' OR label CONTAINS[c] 'MeshCore'")).firstMatch, 30,
             "no answer to ver")
        keep("ct-repeater-console")
    }

    /// CT-7: crest's route, You → hub → crest: the hub's leg from its own signal, crest's not
    /// measured until a trace measures it; no signal of its own; its position; Show on the map.
    func testCT7_TraceToANode() {
        launch()
        tab(.nodes)
        waitLabel(wait(nodeRow("hub"), 30, "no hub"), contains: " dB", 30)
        openNode("crest")
        wait(element(tr("How it reaches you")), 5, "no route")
        let you = tr("You")
        let hubLeg = app.descendants(matching: .any).matching(NSPredicate(format: "label BEGINSWITH %@", tr("%@ to %@, %@", you, "hub", ""))).firstMatch
        let crestLeg = app.descendants(matching: .any).matching(NSPredicate(format: "label BEGINSWITH %@", tr("%@ to %@, %@", "hub", "crest", ""))).firstMatch
        wait(hubLeg, 5, "no leg to the hub")
        XCTAssertTrue(hubLeg.label.hasSuffix(" dB"), hubLeg.label)
        XCTAssertEqual(crestLeg.label, tr("%@ to %@, %@", "hub", "crest", tr("not measured")))
        XCTAssertTrue(element(tr("Not traced yet")).exists, "said traced")
        // Heard only through the hub: no signal of its own.
        let noSignal = tr("Heard only through repeaters: no signal of its own. The route above shows how it reaches you.")
        reveal(element(noSignal))
        XCTAssertTrue(containing("44.90980, 5.46880").exists, "crest's position")
        reveal(button(tr("Trace the path")), up: false)
        button(tr("Trace the path")).tap()
        // Crest's leg, measured now (about −3.0 dB).
        waitLabel(crestLeg, contains: " dB", 30)
        wait(containing(prefix("Traced %@ · back in %@ s · %@ s of airtime")), 5, "the trace's figures")
        XCTAssertTrue(button(tr("Trace again")).exists, "no Trace again")
        keep("ct-trace")
        // The map.
        button(tr("Show on the map")).tap()
        let map = app.tabBars.buttons.element(boundBy: Tab.map.rawValue)
        wait(map, 5)
        XCTAssertTrue(map.isSelected, "not on the map")
    }

    // MARK: CT-14, a node's readings (§16.7)

    /// The readings card's button: Ask, Ask again or Log in.
    private var readingsAction: XCUIElement { app.buttons["readingsAction"] }

    /// A row of the readings card, read as one: "Battery, 3.92 V, about 69 %".
    private func readingRow(_ label: String, _ value: String) -> XCUIElement {
        element(label + ", " + value.replacingOccurrences(of: " · ", with: ", "))
    }

    /// Scrolls until `e` is clear of the navigation bar and the tab bar: under them it counts as
    /// hittable, but a tap goes to the bar.
    private func clearOfBars(_ e: XCUIElement, file: StaticString = #filePath, line: UInt = #line) {
        let top = app.navigationBars.firstMatch.frame.maxY + 8
        let bottom = (app.tabBars.firstMatch.exists ? app.tabBars.firstMatch.frame.minY : app.frame.maxY) - 8
        for _ in 0..<8 {
            let f = e.frame
            if f.minY >= top && f.maxY <= bottom { return }
            let start = app.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5))
            start.press(forDuration: 0.05, thenDragTo: start.withOffset(CGVector(dx: 0, dy: f.minY < top ? 160 : -160)))
            pause(0.5)
        }
        XCTAssertTrue(e.frame.minY >= top && e.frame.maxY <= bottom, "\(e) under a bar", file: file, line: line)
    }

    /// A decimal as the run's language writes it: "3,92" in French.
    private func num(_ text: String) -> String { french ? text.replacingOccurrences(of: ".", with: ",") : text }

    /// CT-14: crest answers only those logged in: its card says so and Log in goes to its login
    /// card; logged in as a guest, Ask gives its battery and temperature with when they came;
    /// Ask again too, and two readings draw the battery's line over 7 days.
    func testCT14_CrestReadings() {
        launch(["-quietDemo", "YES"])
        openNode("crest")
        wait(element(tr("A repeater answers only those logged in, as a guest at least.")), 10, "not said to log in")
        XCTAssertEqual(readingsAction.label, tr("Log in"))
        XCTAssertFalse(containing(prefix("Sent %@")).exists, "asked by opening the page")
        clearOfBars(readingsAction)
        keep("ct-readings-login")
        readingsAction.tap()
        let field = app.secureTextFields[tr("Admin or guest password")]
        focus(field)
        field.typeText("hello")
        app.buttons["serverLogin"].tap()
        wait(element(tr("Logged in as a guest")), 30, "not logged in as a guest")
        waitLabel(readingsAction, contains: tr("Ask"), 10)
        clearOfBars(readingsAction)
        readingsAction.tap()
        let battery = readingRow(tr("Battery"), tr("%@ V · about %lld %%", num("3.92"), 69))
        wait(battery, 30, "no battery")
        XCTAssertTrue(readingRow(tr("Temperature"), "4 °C").exists, "no temperature")
        XCTAssertTrue(containing(prefix("Sent %@")).exists, "not said when")
        keep("ct-readings")
        // Again: two readings, the battery's line.
        waitLabel(readingsAction, contains: tr("Ask again"), 10)
        clearOfBars(readingsAction)
        readingsAction.tap()
        let chart = tr("Battery over 7 days, from %@ to %@ V", "#", "#").components(separatedBy: "#")[0]
        wait(containing(chart), 30, "no battery line after a second reading")
        clearOfBars(element(tr("Readings it sends")))
        keep("ct-readings-chart")
    }

    /// CT-14: Marc, with the stock app, never answers: "no answer", said for a person.
    func testCT14_MarcNeverAnswers() {
        launch(["-quietDemo", "YES"])
        openNode("Marc")
        wait(element(tr("Ask it for its battery and temperature.")), 10, "nothing to ask")
        clearOfBars(readingsAction)
        readingsAction.tap()
        wait(element(tr("No answer. Someone with the MeshCore app answers only if they allow it.")), 40, "no answer not said")
        clearOfBars(readingsAction)
        keep("ct-readings-no-answer")
    }

    /// CT-14: Lucie, heard but not on the radio, cannot be asked: said, and no button.
    func testCT14_LucieCannotBeAsked() {
        launch(["-quietDemo", "YES"])
        openNode("Lucie")
        clearOfBars(wait(readingsAction, 10, "nothing to ask"))
        readingsAction.tap()
        wait(element(tr("Your radio can’t ask it: it isn’t one of its contacts.")), 10, "not said")
        XCTAssertFalse(readingsAction.exists, "a button to ask again")
    }

    /// The request card's line, once Camille's first message arrived as a request and the card
    /// is read (its notification comes and goes meanwhile; a tap while the card is still being
    /// laid out can be lost).
    private func request() -> XCUIElement {
        let card = wait(containing(tr("Someone using %@’s radio wants to write to you", "Camille")), Self.camilleTimeout,
                        "no request from Camille")
        pause(3)
        return card
    }

    /// CT-8: Camille's first message, with requests on, is a request; accepted, her chat opens
    /// with it.
    func testCT8_RequestAccepted() {
        launch(["-requestDemo", "YES"])
        _ = request()
        waitText("Hi! I'm at the refuge", 5)
        XCTAssertFalse(row("Camille").exists, "a request already has a conversation")
        keep("ct-request")
        button(tr("Accept")).tap()
        wait(composer, 10, "her chat did not open")
        waitText("Hi! I'm at the refuge", 5)
        back()
        XCTAssertFalse(containing(tr("Someone using %@’s radio wants to write to you", "Camille")).exists, "the card stayed")
        wait(row("Camille"), 5, "no row for Camille")
    }

    /// CT-9: a request blocked goes, and its identity is listed in Settings › Blocked; one
    /// deleted goes, with no conversation.
    func testCT9_RequestBlocked() {
        launch(["-requestDemo", "YES"])
        _ = request()
        let block = button(tr("Block"))
        XCTAssertTrue(block.isHittable, "Block: \(block.frame)")
        block.tap()
        pause(1)
        keep("ct-request-block-tapped")
        waitGone(containing(tr("Someone using %@’s radio wants to write to you", "Camille")), 5, "the card stayed")
        XCTAssertFalse(row("Camille").exists)
        tab(.settings)
        let blocked = app.buttons.matching(NSPredicate(format: "label BEGINSWITH %@", tr("Blocked"))).firstMatch
        reveal(blocked)
        blocked.tap()
        wait(element(tr("Someone using %@’s radio", "Camille")), 5, "the blocked request is not listed")
        keep("ct-request-blocked")
    }

    func testCT9_RequestDeleted() {
        launch(["-requestDemo", "YES"])
        _ = request()
        button(tr("Delete")).tap()
        waitGone(containing(tr("Someone using %@’s radio wants to write to you", "Camille")), 5, "the card stayed")
        XCTAssertFalse(row("Camille").exists, "a deleted request left a conversation")
        XCTAssertFalse(containing("Hi! I'm at the refuge").exists, "its message stayed")
    }

    /// CT-10: Camille blocked: her answer is lost and she is listed in Settings › Blocked;
    /// unblocked there, her answers come again at once (D-11: her phone's retry of the one
    /// written while blocked is acknowledged and dropped, so it moves on).
    func testCT10_BlockAndUnblock() {
        // At the wall clock's pace, and she answers 20 s after our text reaches her: the block
        // comes before she writes her answer, however slow the machine (B-ios-8). An answer
        // she wrote before the block is not hers "while blocked": it comes after the unblock
        // (ADR-0034, by her own timestamp), as it did on CI when the block took 8 s.
        launch(["-open", "Camille", "-quietDemo", "YES", "-answerAfter", "20", "-demoClock", "YES"], speed: 1)
        wait(keyButton, Self.camilleTimeout, "no session with Camille")
        send("Are you still at the refuge?")
        // Blocked before she writes her answer.
        button(tr("More")).tap()
        button(tr("Block")).tap()
        let confirm = app.buttons.matching(NSPredicate(format: "label == %@", tr("Block"))).element(boundBy: 0)
        wait(confirm, 5).tap()
        wait(containing(tr("You blocked %@. Nothing they send reaches this phone.", "Camille")), 5, "no blocked bar")
        XCTAssertFalse(composer.exists, "a composer for someone blocked")
        pause(25)
        XCTAssertFalse(containing("Copy: \"Are you still").exists, "her answer arrived while blocked")
        keep("ct-blocked")
        back()
        tab(.settings)
        let blocked = app.buttons.matching(NSPredicate(format: "label BEGINSWITH %@", tr("Blocked"))).firstMatch
        reveal(blocked)
        blocked.tap()
        wait(element("Camille"), 5, "Camille not listed as blocked")
        button(tr("Unblock")).tap()
        wait(element(tr("Nobody is blocked.")), 5)
        tab(.messages)
        openChat("Camille", 5)
        send("Back again")
        // Her phone's acknowledgement reaches us again, and her answer. Each goes through the
        // air's retries, a round every `W` and up to 8 rounds unanswered (§7.1, §7.4): a minute
        // was not always enough. Counted on the demo's clock (at this speed, the wall clock's).
        let back = message("Back again")
        var status = "(gone)"
        waitDemo(180, "“\(status)” not delivered") {
            if back.exists { status = back.label }
            return status.contains(tr("Delivered"))
        }
        waitDemo(containing("Copy: \"Back again\""), 180, "no answer to “Back again”")
        keep("ct-unblocked")
        // What she sent while blocked stays lost, as Settings › Blocked says (B-ios-8).
        pause(10)
        XCTAssertFalse(containing("Copy: \"Are you still").exists, "what she sent while blocked came later")
    }

    /// F-1: a contact's MeshCore public key (Camille, with lichen, under her safety number;
    /// Marc, MeshCore only, in his chat's info) and a node's (hub), each copied whole.
    func testF1_ContactAndNodeKeys() {
        launch(["-open", "Camille", "-quietDemo", "YES"])
        openedCamille()
        keyButton.tap()
        let camille = checkMeshCoreKey(meshcoreKey)
        keep("ct-meshcore-key")
        button(tr("Close")).tap()
        back()
        openChat("Marc", 30)
        button(tr("MeshCore · not lichen-encrypted")).tap()
        let marc = checkMeshCoreKey(meshcoreKey)
        XCTAssertNotEqual(marc, camille)
        button(tr("Done")).tap()
        back()
        tab(.nodes)
        wait(app.buttons.matching(NSPredicate(format: "label BEGINSWITH 'hub'")).firstMatch, 20, "no hub").tap()
        let hub = checkMeshCoreKey(meshcoreKey)
        XCTAssertFalse([camille, marc].contains(hub), "the hub's key is someone else's")
        // The last one copied is the one pasted.
        XCTAssertEqual(pasted(), hub, "not the key copied")
    }

    /// CT-11: the radio files Marc as a repeater (`-marcRepeater`): he is not one of the
    /// people, and his MeshCore chat keeps its row, under his name.
    func testCT11_PersonFiledAsARepeaterKeepsTheirRow() {
        launch(["-marcRepeater", "YES"])
        let marc = wait(row("Marc"), 90, "no row for Marc")
        XCTAssertTrue(marc.label.contains("MeshCore"), marc.label)
        marc.tap()
        wait(composer, 10, "Marc did not open")
        wait(app.navigationBars.firstMatch.descendants(matching: .any).matching(NSPredicate(format: "label BEGINSWITH 'Marc'")).firstMatch, 5)
        keep("ct-marc-repeater")
        back()
        button(tr("New message")).tap()
        wait(contact("Camille"), 10)
        XCTAssertFalse(contact("Marc").exists, "Marc listed with the people")
    }

    // MARK: CT-12, a MeshCore contact from a link (F-2, ADR-0047)

    /// A companion's signed advert, as the stock app shares it (`testdata/meshcore/mesh.toml`,
    /// `link-companion`: test keys).
    static let signedLink = "meshcore://110083b6ede3fdf630ceafd3fc187fb096c9cc228e15f1f88749c1efd4e963e7b2724002df6a7e1bae75dd2c45634dddd2c8bc8358fbcaec698e8de8bce1772a5e92d9e3484adb0cf917bddc6c591c3c5113bffdeaa95f4653ab8ec4c9b7424b4717a383180a8148757420726164696f"
    /// A repeater's signed advert that came to the sharing radio through 2 repeaters
    /// (`mesh.toml`, `link-with-2-hop-path`).
    static let twoHopLink = "meshcore://11025a779b3307f6ab98064f997635bf58e102adaceee753b26d5372c328d234a2a855af4602df6a778ef066d76f220fc37acd532b34c9956551adc36e4e0d48dbc53714291572dabd9426998b59a773c6c9fda79d93f6d3098bf1483e165433463b2ac9659feb0a826372657374"
    /// A contact code (`mesh.toml`, the F-1 vectors): unsigned.
    static let contactCode = "meshcore://contact/add?name=Example+Contact&public_key=9cd8fcf22a47333b591d96a2b848b73f457b1bb1a3ea2453a885f9e5787765b1&type=1"

    /// Add a contact › Scan, once the radio is up (its code shows), with `text` in the paste
    /// field and Add tapped.
    private func pasteLink(_ text: String, radioUp: Bool = true, file: StaticString = #filePath, line: UInt = #line) {
        wait(app.navigationBars[tr("Add a contact")], 10, "no contact screen", file: file, line: line)
        if radioUp { wait(element(tr("Your contact code")), 20, "the radio is not up", file: file, line: line) }
        // Tapped again if it did not switch: on a slow machine (CI) the segmented control can
        // lose a tap on a sheet just shown (the thumb lifts, then settles back on My code).
        let scan = app.buttons[tr("Scan")]
        let pasteField = app.textFields["pasteCode"]
        for _ in 0..<3 where !pasteField.exists {
            scan.tap()
            _ = pasteField.waitForExistence(timeout: 3)
        }
        let field = wait(pasteField, 2, "no paste field", file: file, line: line)
        field.tap()
        field.typeText(text)
        button(tr("Add")).tap()
    }

    /// CT-12: a signed link pasted shows its card (name, kind, key, signed, how the sharer's
    /// radio heard it); Add puts the contact among the people and opens their MeshCore chat;
    /// pasted again, it is already on the radio and its chat opens.
    func testCT12_AddAMeshCoreContactFromASignedLink() {
        launch(["-screen", "contact", "-quietDemo", "YES"])
        // A node the sharer's radio heard through 2 repeaters: said on its card; Cancel.
        pasteLink(Self.twoHopLink)
        wait(element("crest"), 5, "no card for the 2-hop link")
        XCTAssertTrue(element(tr("Heard by the sharer’s radio: through %lld repeaters", 2)).exists, "no hops line")
        keep("ct-link-two-hops")
        button(tr("Cancel")).tap()
        pasteLink(Self.signedLink, radioUp: false)
        wait(element("Hut radio"), 5, "no card for the link")
        XCTAssertTrue(element(tr("Heard by the sharer’s radio: directly")).exists, "no line for a link heard directly")
        XCTAssertTrue(element(tr("Companion")).exists, "no kind on the card")
        // The key row (shown in groups of 8) reads out its 64 digits.
        XCTAssertEqual(element(tr("MeshCore public key")).value as? String,
                       "83b6ede3fdf630ceafd3fc187fb096c9cc228e15f1f88749c1efd4e963e7b272")
        XCTAssertTrue(element(tr("Signed by its radio.")).exists, "not said to be signed")
        XCTAssertFalse(element(tr("Not signed: anyone can write such a code.")).exists, "said to be unsigned")
        keep("ct-link-card")
        button(tr("Add to the radio")).tap()
        wait(element(tr("Added to your radio.")), 10, "not added")
        keep("ct-link-added")
        button(tr("Open the chat")).tap()
        wait(composer, 10, "the chat did not open")
        wait(app.navigationBars.firstMatch.descendants(matching: .any).matching(NSPredicate(format: "label BEGINSWITH 'Hut radio'")).firstMatch, 5,
             "not Hut radio's chat")
        back()
        // Among the people, as a MeshCore chat.
        button(tr("New message")).tap()
        wait(contact("Hut radio"), 10, "not among the people")
        app.buttons[tr("Cancel")].firstMatch.tap()

        // Pasted again: already on the radio, and its chat opens.
        button(tr("Add a contact")).tap()
        pasteLink(Self.signedLink)
        wait(element("Hut radio"), 5, "no card for the link")
        button(tr("Add to the radio")).tap()
        wait(element(tr("Already on your radio.")), 5, "not said to be there already")
        keep("ct-link-already")
        button(tr("Open the chat")).tap()
        wait(composer, 10, "the chat did not open")
    }

    /// CT-12: a signed link with one hex digit changed (its timestamp) is refused as changed or
    /// made up, with no card and nothing added.
    func testCT12_AChangedLinkIsRefused() {
        launch(["-screen", "contact", "-quietDemo", "YES"])
        let changed = Self.signedLink.replacingOccurrences(of: "4002df6a", with: "4102df6a")
        XCTAssertNotEqual(changed, Self.signedLink)
        pasteLink(changed)
        let refused = wait(element(tr("This link was changed or made up: its signature doesn’t match.")), 5, "not refused")
        XCTAssertLessThan(refused.frame.height, 100, "the banner is stretched: \(refused.frame)")
        XCTAssertFalse(button(tr("Add to the radio")).exists, "a card for a changed link")
        keep("ct-link-bad-signature")
    }

    /// CT-12: a contact code is read as unsigned, and added.
    func testCT12_AContactCodeIsAddedUnsigned() {
        launch(["-screen", "contact", "-quietDemo", "YES"])
        pasteLink(Self.contactCode)
        wait(element("Example Contact"), 5, "no card for the code")
        XCTAssertTrue(element(tr("Not signed: anyone can write such a code.")).exists, "not said to be unsigned")
        XCTAssertFalse(element(tr("Signed by its radio.")).exists, "said to be signed")
        XCTAssertFalse(containing(tr("Heard by the sharer’s radio: directly")).exists, "a hops line for a code")
        keep("ct-link-unsigned")
        button(tr("Add to the radio")).tap()
        wait(element(tr("Added to your radio.")), 10, "not added")
        button(tr("Close")).tap()
        button(tr("New message")).tap()
        wait(contact("Example Contact"), 10, "not among the people")
    }

    /// CT-12: with the radio gone (`-offline`: the demo radio drops 3 s in), the link's card
    /// says to connect it first.
    func testCT12_ALinkWithNoRadio() {
        launch(["-screen", "contact", "-offline", "12", "-quietDemo", "YES"])
        wait(element(tr("Your contact code")), 20, "the radio did not come up")
        pause(4)
        pasteLink(Self.signedLink, radioUp: false)
        wait(element("Hut radio"), 5, "no card for the link")
        button(tr("Add to the radio")).tap()
        wait(element(tr("Connect your radio first")), 5, "not told to connect the radio")
    }

    // MARK: CT-13, New conversation when the radio knows many people (F-4)

    /// Any of the elements `query` finds can be tapped: on screen, not behind the sheet.
    private func anyHittable(_ query: XCUIElementQuery) -> Bool {
        query.allElementsBoundByIndex.contains { $0.exists && $0.isHittable }
    }

    /// The people filter's segment for `key` ("All (%lld)"), whatever its count.
    private func segment(_ key: String) -> XCUIElement {
        let prefix = String(tr(key, 0).dropLast(2))
        return app.buttons.matching(NSPredicate(format: "label BEGINSWITH %@", prefix)).firstMatch
    }

    /// CT-13: with 200 more people on the radio, the three actions and Public are reachable
    /// without scrolling; the filter shows the MeshCore people only, then the lichen ones only.
    func testCT13_NewConversationWithManyPeople() {
        launch(["-manyContacts", "200", "-quietDemo", "YES"])
        // Camille's identity known (her first message): one lichen person.
        wait(row("Camille"), Self.camilleTimeout, "no Camille")
        button(tr("New message")).tap()
        wait(app.navigationBars[tr("New conversation")], 5, "no New conversation")
        for label in [tr("New group"), tr("Join a channel"), tr("Add a contact")] {
            XCTAssertTrue(anyHittable(app.buttons.matching(NSPredicate(format: "label == %@", label))),
                          "\(label) not reachable without scrolling")
        }
        XCTAssertTrue(anyHittable(app.buttons.matching(NSPredicate(format: "label BEGINSWITH 'Public'"))),
                      "Public not reachable without scrolling")
        keep("ct-new-conversation-many")
        segment("MeshCore (%lld)").tap()
        wait(contact("Radio 001"), 5, "no MeshCore person under MeshCore")
        XCTAssertFalse(contact("Camille").exists, "Camille among the MeshCore people")
        keep("ct-new-conversation-meshcore")
        segment("lichen (%lld)").tap()
        wait(contact("Camille"), 5, "no Camille under lichen")
        XCTAssertFalse(contact("Radio 001").exists, "a MeshCore person among the lichen ones")
    }

    /// CT-13: `#tag` in the search: "Join #tag" joins the hashtag channel and opens it; typed
    /// again, the row is that channel. Not a hashtag: no such row.
    func testCT13_HashtagJoinsAndOpens() {
        launch(["-quietDemo", "YES"])
        wait(element(tr("%@ connected", "ThinkNode M5")), 20, "no radio")
        button(tr("New message")).tap()
        // The radio's channels read (joining needs them): Public listed.
        wait(app.buttons.matching(NSPredicate(format: "label BEGINSWITH 'Public'")).firstMatch, 20, "no channels")
        var search = wait(app.searchFields.firstMatch, 5, "no search field")
        search.tap()
        search.typeText("#two words")
        pause(1)
        XCTAssertFalse(button(tr("Join %@", "#two")).exists, "a row to join a tag with a space")
        // Erased, then a tag.
        search.typeText(String(repeating: XCUIKeyboardKey.delete.rawValue, count: "#two words".count))
        search.typeText("#Ridge")
        wait(button(tr("Join %@", "#ridge")), 5, "no row to join #ridge").tap()
        wait(app.navigationBars.firstMatch.descendants(matching: .any).matching(NSPredicate(format: "label BEGINSWITH '#ridge'")).firstMatch,
             10, "the channel did not open")
        back()
        button(tr("New message")).tap()
        search = wait(app.searchFields.firstMatch, 5, "no search field")
        search.tap()
        search.typeText("#ridge")
        let ridge = app.buttons.matching(NSPredicate(format: "label BEGINSWITH '#ridge'"))
        wait(ridge.firstMatch, 5, "the channel is not the row")
        XCTAssertFalse(button(tr("Join %@", "#ridge")).exists, "offered to join it again")
        // Once: on top, not again under the channels (Messages behind the sheet aside).
        XCTAssertEqual(ridge.allElementsBoundByIndex.filter { $0.isHittable }.count, 1, "the channel shown twice")
        keep("ct-new-conversation-hashtag")
    }

    /// CT-13: New group is its own screen: a name, a lichen contact found by its search,
    /// Create; the group's chat opens, two members in it.
    func testCT13_NewGroupScreen() {
        launch(["-manyContacts", "200", "-quietDemo", "YES"])
        wait(row("Camille"), Self.camilleTimeout, "no Camille")
        button(tr("New message")).tap()
        wait(button(tr("New group")), 5, "no New group").tap()
        wait(app.navigationBars[tr("New group")], 5, "New group is not a screen of its own")
        // Only lichen people can be members.
        XCTAssertFalse(app.buttons.matching(NSPredicate(format: "label BEGINSWITH 'Radio 001'")).firstMatch.exists,
                       "a MeshCore person offered as a member")
        let field = app.textFields[tr("Group name")]
        field.tap()
        field.typeText("Summit team")
        let search = wait(app.searchFields.firstMatch, 5, "no search field")
        search.tap()
        search.typeText("Cam")
        wait(app.buttons.matching(NSPredicate(format: "label BEGINSWITH 'Camille'")).firstMatch, 5, "Camille not found").tap()
        keep("ct-new-group")
        button(tr("Create group")).tap()
        wait(composer, 10, "the group did not open")
        wait(app.navigationBars.firstMatch.descendants(matching: .any).matching(NSPredicate(format: "label BEGINSWITH 'Summit team'")).firstMatch,
             5, "not the group's chat")
        XCTAssertTrue(containing(tr("%lld members", 2)).exists, "not two members")
    }
}
