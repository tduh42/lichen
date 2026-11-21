import XCTest

/// MeshCore chats and channels (docs/testing.md §5), with the demo's stock-app users: Marc, who
/// writes in plain and answers once, and Lucie, who posts on Public.
final class MeshCoreTests: LichenUITest {
    private let marc = "Hi! Marc from the refuge. Are you on lichen? I only have the MeshCore app."

    /// MC-1: Marc's plain message arrives in his MeshCore chat, marked as such; our answer
    /// reaches his radio, and he answers.
    func testMC1_PlainChatWithAStockUser() {
        launch(["-open", "Marc", "-sayPlain", "Yes, both: lichen and MeshCore"])
        wait(composer, 30, "his chat did not open")
        wait(textOf(marc), 30, "no message from Marc")
        XCTAssertTrue(containing(tr("MeshCore · not lichen-encrypted")).exists, "not said to be a MeshCore chat")
        let answer = wait(message("Yes, both: lichen and MeshCore"), 10, "our answer is not shown")
        waitLabel(answer, contains: tr("Delivered"), 60)
        waitText("It works! See you tonight", 60)
        keep("mc-plain")
        back()
        let row = wait(row("Marc"), 5)
        XCTAssertTrue(row.label.contains(tr("MeshCore")), row.label)
    }

    /// MC-2: Public: Lucie's post with her name; a post of ours goes out to the channel.
    func testMC2_PublicChannelReadAndPost() {
        launch(["-open", "Public"])
        wait(composer, 30, "Public did not open")
        wait(containing("Fog on the Pas de la Ville"), 40, "no post from Lucie")
        XCTAssertTrue(element("Lucie").exists || containing("Lucie").exists, "the post's author is not named")
        XCTAssertTrue(containing(tr("MeshCore channel · not lichen-encrypted")).exists)
        send("Anyone on the ridge tonight?")
        waitLabel(message("Anyone on the ridge tonight?"), contains: tr("Sent"), 30)
        keep("mc-public")
    }

    /// MC-3: a channel's code shows after its warning and holds the channel; a code scanned
    /// (`-scanChannel`, the camera's stand-in) joins its channel.
    func testMC3_ChannelCodeShowAndJoin() throws {
        let code = "meshcore://channel/add?name=Ridge&secret=0123456789abcdef0123456789abcdef"
        launch(["-open", "Public", "-scanChannel", code])
        wait(composer, 30, "Public did not open")
        button(tr("Channel details")).tap()
        cardButton(tr("Share by QR code")).tap()
        wait(containing(tr("Whoever scans it joins")), 5, "no warning before the code")
        XCTAssertFalse(element(tr("QR code of %@", "Public")).exists, "the code shows before the warning")
        button(tr("Show the code")).tap()
        let qr = wait(element(tr("QR code of %@", "Public")), 5, "no code")
        let text = try XCTUnwrap(OnboardingTests.decodeQR(qr.screenshot().image), "the code does not decode")
        XCTAssertTrue(text.hasPrefix("meshcore://channel/add?name=Public"), text)
        keep("mc-channel-code")
        button(tr("Done")).tap()
        if button(tr("Done")).exists { button(tr("Done")).tap() }
        back()
        // A code scanned: its channel joined, and opened.
        button(tr("New message")).tap()
        button(tr("Join a channel")).tap()
        cardButton(tr("Scan a channel's QR code")).tap()
        wait(element("Ridge"), 5, "the scanned channel is not shown")
        button(tr("Join the channel")).tap()
        wait(app.navigationBars.firstMatch.descendants(matching: .any).matching(NSPredicate(format: "label BEGINSWITH 'Ridge'")).firstMatch,
             10, "the joined channel did not open")
        back()
        wait(row("Ridge"), 5, "no row for the joined channel")
    }
}
