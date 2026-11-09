import Foundation
import UserNotifications

/// Local notifications for received messages and SOS: posted as they arrive (the app runs in
/// the background while the radio is connected, ADR-0006), shown unless their conversation is
/// on screen; a tap opens it.
@MainActor
final class Notifier: NSObject, UNUserNotificationCenterDelegate {
    /// The conversation on screen, if any.
    var visibleChat: ChatID?
    /// Opens a conversation (a tap on its notification).
    var onOpen: ((ChatID) -> Void)?

    override init() {
        super.init()
        UNUserNotificationCenter.current().delegate = self
    }

    /// Asks once (after the welcome screens).
    func requestAuthorization() {
        UNUserNotificationCenter.current().requestAuthorization(options: [.alert, .sound, .badge]) { _, _ in }
    }

    func message(_ chat: ChatID, title: String, body: String) {
        post(chat, title: title, body: body, sos: false)
    }

    func sos(_ chat: ChatID, title: String, body: String) {
        post(chat, title: title, body: body, sos: true)
    }

    private func post(_ chat: ChatID, title: String, body: String, sos: Bool) {
        let c = UNMutableNotificationContent()
        c.title = title
        c.body = body
        // Critical alerts need an entitlement Apple grants case by case; lichen has none, so an
        // SOS is a normal notification marked time-sensitive (recheck R10). The system shows it
        // as active when the time-sensitive capability is not there either.
        c.sound = .default
        c.interruptionLevel = sos ? .timeSensitive : .active
        c.threadIdentifier = Self.thread(chat)
        c.userInfo = Self.info(chat)
        let r = UNNotificationRequest(identifier: UUID().uuidString, content: c, trigger: nil)
        UNUserNotificationCenter.current().add(r) { error in
            if let error { NSLog("lichen: notification not posted: \(error.localizedDescription)") }
        }
    }

    /// Removes the notifications of a conversation that was opened.
    func clear(_ chat: ChatID) {
        let thread = Self.thread(chat)
        let center = UNUserNotificationCenter.current()
        center.getDeliveredNotifications { delivered in
            let ids = delivered.filter { $0.request.content.threadIdentifier == thread }.map(\.request.identifier)
            center.removeDeliveredNotifications(withIdentifiers: ids)
        }
    }

    private static func thread(_ chat: ChatID) -> String {
        switch chat {
        case let .contact(key): "c" + key.hex
        case let .group(gid): "g" + gid.hex
        }
    }

    private static func info(_ chat: ChatID) -> [String: String] {
        switch chat {
        case let .contact(key): ["kind": "c", "id": key.hex]
        case let .group(gid): ["kind": "g", "id": gid.hex]
        }
    }

    private nonisolated static func chat(from info: [AnyHashable: Any]) -> ChatID? {
        guard let kind = info["kind"] as? String, let hex = info["id"] as? String, let id = Data(hex: hex) else { return nil }
        return kind == "g" ? .group(gid: id) : .contact(key: id)
    }

    // MARK: UNUserNotificationCenterDelegate

    nonisolated func userNotificationCenter(
        _ center: UNUserNotificationCenter, willPresent notification: UNNotification
    ) async -> UNNotificationPresentationOptions {
        let chat = Self.chat(from: notification.request.content.userInfo)
        let visible = await MainActor.run { chat != nil && chat == visibleChat }
        return visible ? [] : [.banner, .list, .sound]
    }

    nonisolated func userNotificationCenter(
        _ center: UNUserNotificationCenter, didReceive response: UNNotificationResponse
    ) async {
        guard let chat = Self.chat(from: response.notification.request.content.userInfo) else { return }
        await MainActor.run { onOpen?(chat) }
    }
}
