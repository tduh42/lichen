import XCTest

/// Media over LoRa (docs/testing.md §6), with Camille: she asks for the full variant of a
/// photo we send, answers a voice note with hers and a sticker with one, and sends a photo of
/// the refuge (about 130 s into the demo).
final class MediaTests: LichenUITest {
    /// Her chat open, her session up (the quiet demo: her answers come in seconds). A sheet
    /// the chat opens on (`-screen file`) hides it: then `opened` is false and the test waits
    /// for the sheet.
    private func openCamille(_ arguments: [String] = [], quiet: Bool = true, opened: Bool = true,
                             file: StaticString = #filePath, line: UInt = #line) {
        launch(["-open", "Camille"] + (quiet ? ["-quietDemo", "YES"] : []) + arguments)
        if opened { openedCamille(file: file, line: line) }
    }

    /// Our photo's preview, sent from the photo sheet (`-screen media`), with its cost said.
    private func sendPhoto() -> XCUIElement {
        // The sheet opens in her lichen chat, over it.
        openCamille(["-screen", "media"], opened: false)
        wait(app.navigationBars[tr("Send a photo")], 60, "no photo sheet")
        XCTAssertTrue(containing(tr("LoRa preview")).exists, "the LoRa preview is not offered")
        XCTAssertTrue(containing(tr("Airtime left: %@ s. Images are never flooded.", "#").components(separatedBy: "#")[0]).exists,
                      "the cost is not said")
        keep("md-photo-sheet")
        button(tr("Send the preview")).tap()
        return wait(app.scrollViews.buttons.matching(NSPredicate(format: "label BEGINSWITH %@", tr("Photo"))).firstMatch, 10,
                    "the photo is not in the chat")
    }

    /// The card on our photo when Camille asks for its full variant (§13.3).
    private var fullRequest: XCUIElement { app.descendants(matching: .any)["fullRequest"] }

    /// MD-1 and MD-2: the preview goes, its cost said first; Camille asks for the full
    /// variant: the card on the photo; Send, and she thanks us once it is there.
    func testMD1_SendAPhotoAndTheFullOneOnRequest() {
        _ = sendPhoto()
        wait(fullRequest, 60, "no request for the full photo")
        keep("md-full-request")
        fullRequest.buttons[tr("Send")].tap()
        waitGone(fullRequest, 10, "the card stayed")
        waitText("Got the full picture, thanks!", 120)
    }

    /// MD-2: Not now: the card goes and nothing is sent; she asks again three minutes later.
    func testMD2_FullPhotoNotNowThenAskedAgain() {
        _ = sendPhoto()
        wait(fullRequest, 60, "no request for the full photo")
        // Its buttons' words whole: « Pas maintenant » broke as « Pas mainte-nant » side by side
        // with « Envoyer » (B-ios-25); stacked, it has the card's width.
        let notNow = fullRequest.buttons[tr("Not now")]
        if french { XCTAssertGreaterThan(notNow.frame.width, 150, "« \(tr("Not now")) » squeezed: \(notNow.frame)") }
        keep("md-full-request-buttons")
        notNow.tap()
        waitGone(fullRequest, 10, "Not now left the card")
        pause(5)
        XCTAssertFalse(containing("Got the full picture").exists, "the full photo went anyway")
        // Three minutes of the demo's time later (45 s at speed 4), asked again.
        wait(fullRequest, 90, "not asked again")
    }

    /// MD-3, B-ios-20: her photo arrives (a thumbnail), the viewer opens on it (`-screen
    /// photo`), and Save Image in the share sheet puts it in the photo library (the share sheet
    /// had no Save Image: the app did not ask to add to the library).
    func testMD3_ReceiveAPhotoAndOpenTheViewer() {
        openCamille(["-screen", "photo"], quiet: false)
        waitText("The view from the refuge tonight", 120)
        let close = wait(button(tr("Close")), 90, "the viewer did not open")
        keep("md-viewer")
        button(tr("Share")).tap()
        // The system's share sheet: Save Image among its actions.
        let save = app.descendants(matching: .any)
            .matching(NSPredicate(format: "label IN %@", ["Save Image", "Enregistrer l’image", "Enregistrer l'image"])).firstMatch
        wait(save, 10, "no Save Image in the share sheet")
        keep("md-share-sheet")
        save.tap()
        // Adding to the library is asked once per install.
        let allow = XCUIApplication(bundleIdentifier: "com.apple.springboard").buttons
            .matching(NSPredicate(format: "label IN %@", ["Allow", "Autoriser", "Allow Access", "Autoriser l’accès"])).firstMatch
        let asked = Date().addingTimeInterval(5)
        while !allow.exists && Date() < asked { pause(0.5) }
        if allow.exists { allow.tap() }
        // Saved: the sheet is gone, the viewer stays.
        waitGone(save, 10, "the share sheet stayed")
        wait(close, 10, "the viewer did not stay").tap()
    }

    /// MD-4: a voice note (`-screen voiceReview`, the microphone's stand-in): listened to, sent,
    /// delivered; Camille answers with hers, and it plays.
    func testMD4_VoiceReviewSendPlay() {
        openCamille(["-screen", "voiceReview"])
        let listen = wait(button(tr("Listen to what will be sent")), 10, "no recording under review")
        // Its cost: fragments and airtime ("1 fragment · ≈ 2 s of airtime").
        XCTAssertTrue(containing("≈").exists, "the voice note's cost is not said")
        keep("md-voice-review")
        listen.tap()
        wait(button(tr("Stop")), 5, "it does not play").tap()
        button(tr("Send")).tap()
        let notes = app.buttons.matching(NSPredicate(format: "label == %@", tr("Play the voice message")))
        wait(notes.firstMatch, 10, "our voice note is not in the chat")
        // Hers, a moment after ours reaches her.
        let deadline = Date().addingTimeInterval(90)
        while notes.count < 2 && Date() < deadline { pause(2) }
        XCTAssertGreaterThanOrEqual(notes.count, 2, "no voice note from Camille")
        notes.element(boundBy: notes.count - 1).tap()
        wait(button(tr("Stop")), 5, "hers does not play")
        keep("md-voice-sent")
    }

    /// MD-5: a small note goes over LoRa, its cost said; a big document cannot (the bridge is
    /// offered instead, or said to be needed).
    func testMD5_SmallFileSent() {
        openCamille(["-screen", "file"], opened: false)
        wait(app.navigationBars[tr("Send a file")], 60, "no file sheet")
        XCTAssertTrue(containing(tr("Over LoRa")).exists, "LoRa is not offered for a small note")
        keep("md-file-small")
        button(tr("Send over LoRa")).tap()
        waitGone(app.navigationBars[tr("Send a file")], 10, "the sheet stayed")
        let card = wait(app.scrollViews.descendants(matching: .any).matching(NSPredicate(format: "label CONTAINS[c] '.txt'")).firstMatch,
                        10, "the file is not in the chat")
        XCTAssertTrue(card.exists)
    }

    func testMD5_BigFileNotOverLoRa() {
        openCamille(["-screen", "fileBig"], opened: false)
        wait(app.navigationBars[tr("Send a file")], 60, "no file sheet")
        let refused = containing(tr("This file cannot go over LoRa")).exists
        let bridge = containing(tr("Via iroh")).exists
        XCTAssertTrue(refused || bridge, "a 240 KB document neither refused nor offered by the bridge")
        XCTAssertFalse(button(tr("Send over LoRa")).exists && button(tr("Send over LoRa")).isEnabled,
                       "a 240 KB document offered over LoRa")
        keep("md-file-big")
        button(tr("Cancel")).tap()
    }

    /// MD-6: a sticker from the picker shows in the chat; Camille answers with one.
    func testMD6_StickerSentAndAnswered() {
        openCamille()
        button(tr("Attach")).tap()
        wait(button(tr("Sticker")), 5).tap()
        let stickers = app.buttons.matching(NSPredicate(format: "label BEGINSWITH %@", tr("Sticker %@", "")))
        wait(stickers.firstMatch, 10, "no stickers to pick").tap()
        let shown = app.scrollViews.descendants(matching: .any).matching(NSPredicate(format: "label BEGINSWITH %@", tr("Sticker %@", "")))
        wait(shown.firstMatch, 10, "the sticker is not in the chat")
        let deadline = Date().addingTimeInterval(60)
        while shown.count < 2 && Date() < deadline { pause(2) }
        XCTAssertGreaterThanOrEqual(shown.count, 2, "no sticker from Camille")
        keep("md-stickers")
    }
}
