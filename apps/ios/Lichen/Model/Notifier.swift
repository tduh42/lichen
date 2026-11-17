import Foundation
import UserNotifications

/// Local notifications for received messages and SOS: posted as they arrive (the app runs in
/// the background while the radio is connected, ADR-0006), shown unless their conversation is
/// on screen; a tap opens it.
@MainActor
final class Notifier: NSObject, UNUserNotificationCenterDelegate {
    /// The conversation on screen, if any.
    var visibleChat: ChatID?
    /// The app lock's cover is up: the conversation behind it is not on screen.
    var locked: (() -> Bool)?
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

    /// Messages, reactions, invitations, MeshCore and channel posts. With the app lock on, they
    /// say only "New message": no sender, no group, no text on the lock screen.
    func message(_ chat: ChatID, title: String, body: String) {
        if AppLock.hidesNotifications {
            post(chat, title: "lichen", body: String(localized: "New message"), sos: false)
        } else {
            post(chat, title: title, body: body, sos: false)
        }
    }

    /// An SOS keeps its sender and content with the app lock on too: safety comes first.
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
        case let .mesh(key): "m" + key.hex
        case let .channel(id): "h" + id.hex
        case let .room(key): "r" + key.hex
        }
    }

    private static func info(_ chat: ChatID) -> [String: String] {
        switch chat {
        case let .contact(key): ["kind": "c", "id": key.hex]
        case let .group(gid): ["kind": "g", "id": gid.hex]
        case let .mesh(key): ["kind": "m", "id": key.hex]
        case let .channel(id): ["kind": "h", "id": id.hex]
        case let .room(key): ["kind": "r", "id": key.hex]
        }
    }

    private nonisolated static func chat(from info: [AnyHashable: Any]) -> ChatID? {
        guard let kind = info["kind"] as? String, let hex = info["id"] as? String, let id = Data(hex: hex) else { return nil }
        return switch kind {
        case "g": .group(gid: id)
        case "m": .mesh(key: id)
        case "h": .channel(id: id)
        case "r": .room(key: id)
        default: .contact(key: id)
        }
    }

    // MARK: UNUserNotificationCenterDelegate

    // The completion-handler forms, called on the main queue: with the async forms, iOS called
    // the completion wherever the task ended, off the main thread, and UIKit aborted the app
    // when a notification was tapped.

    nonisolated func userNotificationCenter(
        _ center: UNUserNotificationCenter, willPresent notification: UNNotification,
        withCompletionHandler completionHandler: @escaping (UNNotificationPresentationOptions) -> Void
    ) {
        let chat = Self.chat(from: notification.request.content.userInfo)
        DispatchQueue.main.async {
            let visible = MainActor.assumeIsolated {
                chat != nil && chat == self.visibleChat && self.locked?() != true
            }
            completionHandler(visible ? [] : [.banner, .list, .sound])
        }
    }

    nonisolated func userNotificationCenter(
        _ center: UNUserNotificationCenter, didReceive response: UNNotificationResponse,
        withCompletionHandler completionHandler: @escaping () -> Void
    ) {
        let chat = Self.chat(from: response.notification.request.content.userInfo)
        DispatchQueue.main.async {
            if let chat {
                MainActor.assumeIsolated { self.onOpen?(chat) }
            }
            completionHandler()
        }
    }
}
