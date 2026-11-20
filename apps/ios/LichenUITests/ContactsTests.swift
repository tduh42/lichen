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

    /// Settings › Nodes, then the node `name`.
    private func openNode(_ name: String, file: StaticString = #filePath, line: UInt = #line) {
        tab(.settings)
        let nodes = app.buttons.matching(NSPredicate(format: "label BEGINSWITH %@", tr("Nodes"))).firstMatch
        wait(nodes, 10, "no Nodes", file: file, line: line).tap()
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
        launch(["-open", "Camille", "-quietDemo", "YES"])
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

    /// CT-3: the nodes the radio knows: repeaters and the room with their kind, the positions
    /// they advertise and when they were last heard; Lucie, heard, not added.
    func testCT3_Nodes() {
        launch()
        tab(.settings)
        let nodes = app.buttons.matching(NSPredicate(format: "label BEGINSWITH %@", tr("Nodes"))).firstMatch
        wait(nodes, 10).tap()
        for (name, kind) in [("hub", "Repeater"), ("crest", "Repeater"), ("Refuge board", "Room server"), ("Lucie", "Companion")] {
            let n = wait(app.buttons.matching(NSPredicate(format: "label BEGINSWITH %@", name)).firstMatch, 20, "no node \(name)")
            XCTAssertTrue(n.label.contains(tr(kind)), "\(name): \(n.label)")
        }
        XCTAssertTrue(app.buttons.matching(NSPredicate(format: "label BEGINSWITH 'Lucie'")).firstMatch.label.contains(tr("heard")),
                      "Lucie is not marked as heard")
        // One hop is said in the singular.
        let camille = app.buttons.matching(NSPredicate(format: "label BEGINSWITH 'Camille'")).firstMatch.label
        XCTAssertTrue(camille.contains(tr("%lld hops", 1)), camille)
        XCTAssertFalse(camille.contains(tr("%lld hops", 2).replacingOccurrences(of: "2", with: "1")), camille)
        keep("ct-nodes")
        // The filters.
        app.buttons[tr("Repeaters")].tap()
        XCTAssertFalse(app.buttons.matching(NSPredicate(format: "label BEGINSWITH 'Refuge board'")).firstMatch.exists)
        XCTAssertTrue(app.buttons.matching(NSPredicate(format: "label BEGINSWITH 'hub'")).firstMatch.exists)
        app.buttons[tr("Rooms")].tap()
        XCTAssertTrue(app.buttons.matching(NSPredicate(format: "label BEGINSWITH 'Refuge board'")).firstMatch.exists)
        XCTAssertFalse(app.buttons.matching(NSPredicate(format: "label BEGINSWITH 'hub'")).firstMatch.exists)
        app.buttons[tr("All")].tap()
        // A repeater's page: kind, position, last heard, path.
        app.buttons.matching(NSPredicate(format: "label BEGINSWITH 'crest'")).firstMatch.tap()
        wait(containing(tr("Advertised position")), 5, "no position")
        XCTAssertTrue(containing("44.90980, 5.46880").exists, "crest's position")
        XCTAssertTrue(containing(tr("Last heard %@", "#").components(separatedBy: "#")[0]).exists, "no last heard")
        XCTAssertTrue(containing("hub").exists, "the path to crest goes through hub")
        keep("ct-node-crest")
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
        button(tr("Log in")).tap()
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
        button(tr("Log in")).tap()
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

    /// CT-7: a trace to crest, two hops: each leg's SNR, there and back.
    func testCT7_TraceToANode() {
        launch()
        openNode("crest")
        let trace = button(tr("Trace the path"))
        reveal(trace)
        trace.tap()
        wait(app.staticTexts.matching(NSPredicate(format: "label BEGINSWITH %@", tr("Back in %@ s · SNR measured by each hop, out and back", "")
            .components(separatedBy: " ").prefix(2).joined(separator: " "))).firstMatch, 30, "the trace did not come back")
        // You → hub → crest → hub → You: four legs, each with an SNR.
        let legs = app.staticTexts.matching(NSPredicate(format: "label MATCHES '-?[0-9]+\\\\.[0-9] dB'"))
        XCTAssertEqual(legs.count, 4, "legs: \(legs.allElementsBoundByIndex.map(\.label))")
        keep("ct-trace")
    }

    /// The request card's line, once Camille's first message arrived as a request and the card
    /// is read (its notification comes and goes meanwhile; a tap while the card is still being
    /// laid out can be lost).
    private func request() -> XCUIElement {
        let card = wait(containing(tr("Someone using %@'s radio wants to write to you", "Camille")), Self.camilleTimeout,
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
        XCTAssertFalse(containing(tr("Someone using %@'s radio wants to write to you", "Camille")).exists, "the card stayed")
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
        waitGone(containing(tr("Someone using %@'s radio wants to write to you", "Camille")), 5, "the card stayed")
        XCTAssertFalse(row("Camille").exists)
        tab(.settings)
        let blocked = app.buttons.matching(NSPredicate(format: "label BEGINSWITH %@", tr("Blocked"))).firstMatch
        reveal(blocked)
        blocked.tap()
        wait(element(tr("Someone using %@'s radio", "Camille")), 5, "the blocked request is not listed")
        keep("ct-request-blocked")
    }

    func testCT9_RequestDeleted() {
        launch(["-requestDemo", "YES"])
        _ = request()
        button(tr("Delete")).tap()
        waitGone(containing(tr("Someone using %@'s radio wants to write to you", "Camille")), 5, "the card stayed")
        XCTAssertFalse(row("Camille").exists, "a deleted request left a conversation")
        XCTAssertFalse(containing("Hi! I'm at the refuge").exists, "its message stayed")
    }

    /// CT-10: Camille blocked: her answer is lost and she is listed in Settings › Blocked;
    /// unblocked there, her answers come again.
    func testCT10_BlockAndUnblock() {
        launch(["-open", "Camille", "-quietDemo", "YES"])
        wait(keyButton, Self.camilleTimeout, "no session with Camille")
        send("Are you still at the refuge?")
        // Blocked before her answer comes (about 5 s after it reaches her).
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
        waitLabel(message("Back again"), contains: tr("Delivered"), 120)
        // Her messages come back. B-ios-8 (handed to core): her answer to the new text may not
        // come at all, and what she sent while blocked, said to be lost (Settings › Blocked),
        // comes once unblocked.
        let options = XCTExpectedFailure.Options()
        options.isStrict = false
        XCTExpectFailure("B-ios-8: after an unblock, the old answer comes and the new one may not", options: options) {
            XCTAssertTrue(containing("Copy: \"Back again\"").waitForExistence(timeout: 90), "no answer to Back again")
            XCTAssertFalse(containing("Copy: \"Are you still").exists, "what she sent while blocked came later")
        }
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
}
