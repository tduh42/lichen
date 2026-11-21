import XCTest

/// The system's accessibility audit (`performAccessibilityAudit`, Xcode 15) on the main screens
/// of the demo network: Messages, a conversation, a group, the map, Locate and Settings.
/// Contrast, element labels, hit regions, Dynamic Type, clipped text and traits.
final class AccessibilityAuditTests: XCTestCase {
    override func setUp() {
        continueAfterFailure = true
    }

    /// The demo as the other UI tests run it: 20 s sooner, 4 times faster, as just installed,
    /// no banners (LichenUITest.launch).
    private func launch(_ arguments: [String]) -> XCUIApplication {
        let app = XCUIApplication()
        app.launchArguments = ["-demo", "YES", "-onboarded", "YES", "-demoSkip", "20", "-demoSpeed", "4",
                               "-freshInstall", "YES", "-noBanners", "YES"] + arguments
        app.launch()
        // The notification permission, asked once per install: looked for at the first launch
        // only, without a timed-out wait (XCTest then spends 10 s collecting diagnostics).
        if !Self.permissionLooked {
            Self.permissionLooked = true
            let allow = XCUIApplication(bundleIdentifier: "com.apple.springboard").buttons["Allow"]
            for _ in 0..<10 where !allow.exists { usleep(300_000) }
            if allow.exists { allow.tap() }
        }
        return app
    }

    private static var permissionLooked = false

    /// Audits what is on screen. Left out, as the system's doing and not the app's:
    /// - issues with no element to point at;
    /// - contrast where the content scrolls under the navigation bar or the tab bar (Liquid
    ///   Glass blurs and fades what is behind it), or under the status bar;
    /// - contrast the text's own pixels pass: the audit flags plain text in bubbles and notes
    ///   whose screenshots measure 7:1 to 16:1 (2026-11-17, iOS 27.2 simulator); it is measured
    ///   again here, WCAG's formula on the element's screenshot, against 4.5:1;
    /// - Dynamic Type and clipping inside the navigation bar: the system caps the bar's text
    ///   and shows it whole in the Large Content Viewer.
    /// And, the app's own but audited apart: Dynamic Type and clipping below `squeezedBelow`.
    /// A banner pinned above the messages grows with the text and leaves them little room, so
    /// the audit no longer sees them grow (B-ios-21, a question for the coordinator); they are
    /// audited again with the banner put away.
    private func audit(_ app: XCUIApplication, _ screen: String, squeezedBelow: CGFloat? = nil) throws {
        let navigationBar = app.navigationBars.firstMatch.exists ? app.navigationBars.firstMatch.frame : .null
        let tabBar = app.tabBars.firstMatch.exists ? app.tabBars.firstMatch.frame : .null
        let statusBar = CGRect(x: 0, y: 0, width: app.frame.width, height: 54)
        try app.performAccessibilityAudit(for: .all) { issue in
            guard let element = issue.element else { return true }
            let frame = element.frame
            let underBars = frame.intersects(navigationBar) || frame.intersects(tabBar) || frame.intersects(statusBar)
            let inNavigationBar = navigationBar.contains(frame)
            let ignored: Bool
            switch issue.auditType {
            case .contrast:
                ignored = underBars || (element.elementType == .staticText && (Self.contrast(of: element) ?? 0) >= 4.5)
            case .dynamicType, .textClipped:
                ignored = inNavigationBar || squeezedBelow.map { frame.minY >= $0 } == true
            default: ignored = false
            }
            if !ignored {
                print("Audit \(screen): \(issue.auditType.rawValue) · \(element.elementType.rawValue) · "
                      + "\(element.label) · \(frame) · \(issue.compactDescription)")
            }
            return ignored
        }
    }

    /// WCAG's contrast ratio of an element's screenshot: its most frequent colour (the
    /// background) against the colour farthest from it that covers a few pixels (the text).
    static func contrast(of element: XCUIElement) -> Double? {
        guard let image = element.screenshot().image.cgImage else { return nil }
        let w = image.width, h = image.height
        var pixels = [UInt8](repeating: 0, count: w * h * 4)
        let drawn = pixels.withUnsafeMutableBytes { buffer -> Bool in
            guard let context = CGContext(data: buffer.baseAddress, width: w, height: h, bitsPerComponent: 8,
                                          bytesPerRow: w * 4, space: CGColorSpace(name: CGColorSpace.sRGB)!,
                                          bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue)
            else { return false }
            context.draw(image, in: CGRect(x: 0, y: 0, width: w, height: h))
            return true
        }
        guard drawn else { return nil }
        var counts: [UInt32: Int] = [:]
        for i in stride(from: 0, to: pixels.count, by: 4) {
            counts[UInt32(pixels[i]) << 16 | UInt32(pixels[i + 1]) << 8 | UInt32(pixels[i + 2]), default: 0] += 1
        }
        guard let background = counts.max(by: { $0.value < $1.value })?.key else { return nil }
        func luminance(_ c: UInt32) -> Double {
            func channel(_ v: UInt32) -> Double {
                let s = Double(v) / 255
                return s <= 0.03928 ? s / 12.92 : pow((s + 0.055) / 1.055, 2.4)
            }
            return 0.2126 * channel(c >> 16 & 255) + 0.7152 * channel(c >> 8 & 255) + 0.0722 * channel(c & 255)
        }
        let lb = luminance(background)
        let floor = max(4, w * h / 500)
        return counts.filter { $0.value >= floor }.keys.map { c -> Double in
            let lc = luminance(c)
            return (max(lb, lc) + 0.05) / (min(lb, lc) + 0.05)
        }.max()
    }

    func testMessages() throws {
        let app = launch([])
        let row = app.buttons.matching(NSPredicate(format: "label BEGINSWITH 'Camille'")).firstMatch
        XCTAssertTrue(row.waitForExistence(timeout: 120), "no Camille")
        try audit(app, "messages")
    }

    func testConversation() throws {
        let app = launch(["-open", "Camille", "-say", "On my way up, see you at the refuge tonight", "-quietDemo", "YES"])
        XCTAssertTrue(app.descendants(matching: .any)["composer"].waitForExistence(timeout: 60))
        let encrypted = app.staticTexts.containing(NSPredicate(format: "label BEGINSWITH 'Encrypted session'")).firstMatch
        XCTAssertTrue(encrypted.waitForExistence(timeout: 90), "no session with Camille")
        // The chat settled, her answer in: what changes while the audit runs (a message
        // landing, a status) makes it see text that does not follow Dynamic Type.
        let answer = app.descendants(matching: .any).matching(NSPredicate(format: "label CONTAINS 'Copy: '")).firstMatch
        XCTAssertTrue(answer.waitForExistence(timeout: 60), "no answer from Camille")
        sleep(3)
        // The banner asking to compare safety numbers (B-user-1), then the chat without it.
        let banner = app.descendants(matching: .any)
            .matching(NSPredicate(format: "label BEGINSWITH 'Compare safety numbers with'")).firstMatch
        XCTAssertTrue(banner.exists, "no banner asking to compare")
        try audit(app, "chat, compare banner", squeezedBelow: banner.frame.maxY)
        app.buttons["Later"].tap()
        sleep(2)
        try audit(app, "chat")
    }

    func testGroup() throws {
        let app = launch(["-open", "Rando Vercors"])
        XCTAssertTrue(app.descendants(matching: .any)["composer"].waitForExistence(timeout: 90), "no group")
        sleep(3)
        try audit(app, "group")
    }

    func testMap() throws {
        let app = launch(["-tab", "map"])
        sleep(10)
        try audit(app, "map")
    }

    func testLocate() throws {
        let app = launch(["-tab", "locate", "-sosdemo", "YES"])
        sleep(10)
        try audit(app, "locate")
    }

    func testSettings() throws {
        let app = launch(["-tab", "settings"])
        sleep(5)
        try audit(app, "settings")
        app.swipeUp()
        sleep(1)
        try audit(app, "settings, lower")
    }
}
