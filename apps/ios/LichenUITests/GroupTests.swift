import XCTest

/// Groups (docs/testing.md §7): one of our own, the demo's "Rando Vercors" (Camille, its admin,
/// and Julien), invitations by QR code in both directions, and the post the app's radio misses.
final class GroupTests: LichenUITest {
    private let group = "Rando Vercors"

    /// A group of our own, alone in it, from New message › New group; its chat opens.
    private func createGroup(_ name: String) {
        button(tr("New message")).tap()
        // New group is the list's last section: scrolled to.
        let field = app.textFields[tr("Group name")]
        for _ in 0..<8 where !(field.exists && field.isHittable) { app.collectionViews.firstMatch.swipeUp() }
        wait(field, 5, "no group name field")
        field.tap()
        field.typeText(name)
        let create = button(tr("Create group"))
        // Scrolled to, then still: a tap while the list still moves only stops it.
        if !create.isHittable { reveal(create); pause(1) }
        create.tap()
        if !composer.waitForExistence(timeout: 5), create.exists, create.isHittable { create.tap() }
        wait(composer, 10, "the group did not open")
    }

    /// The group's details (its info button).
    private func details() {
        button(tr("Group details")).tap()
    }

    /// GR-1, B-ios-17: a group created, alone: its chat, its details and its row in Messages,
    /// its one member said in the singular (the details said "1 members").
    func testGR1_CreateAGroup() {
        launch()
        wait(element(tr("%@ connected", "ThinkNode M5")), 20, "no radio")
        createGroup("Sunday hike")
        let title = app.navigationBars.firstMatch.descendants(matching: .any)
            .matching(NSPredicate(format: "label BEGINSWITH 'Sunday hike'")).firstMatch
        wait(title, 5, "the bar does not name the group")
        let one = french ? "1 membre" : "1 member", wrong = french ? "1 membres" : "1 members"
        XCTAssertTrue(containing(one).exists, "not said to be one member")
        keep("gr-created")
        details()
        wait(containing(one), 5, "the details do not say one member")
        XCTAssertFalse(containing(wrong).exists, "“\(wrong)” in the group's details")
        keep("gr-details")
        back()
        wait(composer, 5, "not back in the group")
        back()
        wait(row("Sunday hike"), 5, "no row for the group")
    }

    /// GR-2: our group's invitation QR (`-screen invite`); Léa, next to us, scans it
    /// (`-inviteDemo`), is admitted, and her post arrives. D-3: our own invitation, pasted
    /// back, is said to be ours.
    func testGR2_InviteByQRSomeoneJoins() throws {
        launch(["-screen", "invite", "-inviteDemo", "YES"])
        let qr = wait(app.descendants(matching: .any).matching(NSPredicate(format: "label BEGINSWITH %@",
            tr("Invitation code for %@", "#").components(separatedBy: "#")[0])).firstMatch, 30, "no invitation code")
        let invite = try XCTUnwrap(OnboardingTests.decodeQR(qr.screenshot().image), "the code does not decode")
        XCTAssertTrue(invite.hasPrefix("lichen:1:g:"), invite)
        keep("gr-invite")
        wait(element("Léa"), 60, "Léa did not join")
        button(tr("Done")).tap()
        if app.navigationBars.buttons.count > 0, !composer.exists { back() }
        wait(composer, 10)
        waitText("Hi, it's Léa!", 90)
        // Our own invitation, pasted back (D-3).
        back()
        UIPasteboard.general.string = invite
        button(tr("Add a contact")).tap()
        app.buttons[tr("Scan")].tap()
        let field = wait(app.textFields["pasteCode"], 5, "no paste field")
        field.tap()
        field.press(forDuration: 1.2)
        wait(app.menuItems.matching(NSPredicate(format: "label IN %@", ["Paste", "Coller"])).firstMatch, 5, "no Paste").tap()
        button(tr("Add")).tap()
        wait(containing(tr("This is your own code.")), 5, "our own invitation is not said to be ours")
    }

    /// GR-3: an invitation scanned (`-screen join`, Camille's "Refuge tonight"): joined, the
    /// group opens with its members. B-ios-18: her other group, "Rando Vercors", adds us while
    /// we wait; it was taken for the one joined, said joined and opened.
    func testGR3_JoinByScanningAnInvitation() {
        launch(["-screen", "join", "-joinNow", "YES"])
        let title = app.navigationBars.firstMatch.descendants(matching: .any)
            .matching(NSPredicate(format: "label BEGINSWITH 'Refuge tonight'")).firstMatch
        wait(title, 90, "the group did not open")
        details()
        wait(element("Camille"), 10, "Camille is not a member")
        XCTAssertTrue(element(tr("You")).exists, "we are not a member")
        keep("gr-joined")
    }

    /// GR-4: an invitation received (`-groupInvitationDemo`): its card in Messages; joined,
    /// the group is there.
    func testGR4_InvitationAccepted() {
        launch(["-groupInvitationDemo", "YES"])
        let card = wait(containing(tr("%@ invites you to “%@”", "Camille", group)), 60, "no invitation")
        keep("gr-invitation")
        XCTAssertFalse(row(group).exists, "a group before it is accepted")
        button(tr("Join")).tap()
        waitGone(card, 10, "the card stayed")
        wait(row(group), 30, "the group did not come")
    }

    /// GR-4: declined, the card goes and no group comes.
    func testGR4_InvitationDeclined() {
        launch(["-groupInvitationDemo", "YES"])
        let card = wait(containing(tr("%@ invites you to “%@”", "Camille", group)), 60, "no invitation")
        button(tr("Decline")).tap()
        waitGone(card, 10, "the card stayed")
        pause(10)
        XCTAssertFalse(row(group).exists, "a declined group came")
    }

    /// GR-5: a post of ours goes to the group; Camille's answer comes, under her name, after it.
    func testGR5_PostAndReceive() {
        launch(["-open", group])
        wait(composer, 90, "the group did not open")
        send("On my way to the col")
        waitLabel(message("On my way to the col"), contains: tr("Sent"), 60)
        let answer = waitText("Seen. I'll keep a seat for you by the stove.", 90)
        XCTAssertTrue(answer.label.contains("Camille") || containing("Camille").exists, "her post is not under her name")
        XCTAssertLessThan(message("On my way to the col").frame.minY, answer.frame.minY, "her answer above our post")
        keep("gr-posts")
    }

    /// GR-6: Julien's second post, missed by our radio, comes back marked as arrived late,
    /// after his GIF revealed the gap.
    func testGR6_ALostPostComesBackLate() {
        launch(["-open", group])
        wait(composer, 90, "the group did not open")
        let late = wait(app.scrollViews.descendants(matching: .any)
            .matching(NSPredicate(format: "label CONTAINS 'Mist on the ridge' AND label CONTAINS %@",
                                  tr("%@ · arrived late, written at %@", "#", "#").components(separatedBy: "#")[1])).firstMatch,
            240, "his lost post did not come back marked late")
        XCTAssertTrue(late.exists)
        keep("gr-late")
    }

    /// GR-7: a short voice note to the group: its cost said first, then sent.
    func testGR7_GroupVoiceNote() {
        launch(["-open", group, "-screen", "voiceReview"])
        wait(composer.exists ? composer : button(tr("Listen to what will be sent")), 90, "the group did not open")
        wait(button(tr("Listen to what will be sent")), 30, "no recording under review")
        XCTAssertTrue(containing(tr("Flooded to the group: %@ · ≈ %@ s of our airtime; every repeater adds as much", "#", "#")
            .components(separatedBy: "#")[0]).exists, "the flood's cost is not said")
        keep("gr-voice-review")
        button(tr("Send")).tap()
        let note = wait(app.buttons.matching(NSPredicate(format: "label == %@", tr("Play the voice message"))).firstMatch, 10,
                        "the voice note is not in the group")
        XCTAssertTrue(note.exists)
        waitText(tr("Sent"), 60)
    }

    /// GR-8: our group renamed (the title follows), its members listed, then left: gone from
    /// Messages, and it does not come back.
    func testGR8_RenameMembersLeave() {
        launch()
        wait(element(tr("%@ connected", "ThinkNode M5")), 20, "no radio")
        createGroup("Sunday hike")
        details()
        let name = cardButton(tr("Name"))
        reveal(name)
        name.tap()
        let alert = app.alerts[tr("Rename the group")]
        wait(alert, 5, "no rename")
        let field = alert.textFields.firstMatch
        field.tap()
        field.typeText(String(repeating: XCUIKeyboardKey.delete.rawValue, count: 12) + "Monday hike")
        alert.buttons[tr("Rename")].tap()
        wait(element("Monday hike"), 10, "the new name is not shown")
        XCTAssertTrue(element(tr("You")).exists && element(tr("Admin")).exists, "we are not listed as its admin")
        keep("gr-renamed")
        let leave = button(tr("Leave the group"))
        reveal(leave)
        leave.tap()
        wait(app.buttons.matching(NSPredicate(format: "label CONTAINS %@", tr("Close the group for everyone"))).firstMatch, 5)
            .tap()
        wait(app.tabBars.firstMatch, 5)
        pause(2)
        XCTAssertFalse(row("Monday hike").exists, "a closed group stays in Messages")
        XCTAssertFalse(row("Sunday hike").exists)
    }
}
