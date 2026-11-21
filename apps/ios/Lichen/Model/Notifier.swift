import Foundation
import UserNotifications

/// Local notifications for received messages and SOS: posted as they arrive (the app runs in
/// the background while the radio is connected, ADR-0006), shown unless their conversation is
/// on screen; a tap opens it. A message's notification also answers and marks the conversation
/// read (ADR-0034), unless the app lock hides it.
@MainActor
final class Notifier: NSObject, UNUserNotificationCenterDelegate {
    /// The conversation on screen, if any.
    var visibleChat: ChatID?
    /// The app lock's cover is up: the conversation behind it is not on screen.
    var locked: (() -> Bool)?
    /// Opens a conversation (a tap on its notification).
    var onOpen: ((ChatID) -> Void)?
    /// A text typed in a notification, for its conversation.
    var onReply: ((ChatID, String) -> Void)?
    /// "Mark as read" on a notification.
    var onMarkRead: ((ChatID) -> Void)?

    private static let messageCategory = "message"
    private static let replyAction = "reply"
    private static let readAction = "read"
    /// What a text typed in a notification may hold at most: the core refuses what does not
    /// fit a message anyway.
    private static let replyMax = 4_096

    override init() {
        super.init()
        let center = UNUserNotificationCenter.current()
        center.delegate = self
        // Both need the phone unlocked: whoever holds a locked phone neither answers nor hides
        // messages as its owner.
        let reply = UNTextInputNotificationAction(
            identifier: Self.replyAction, title: String(localized: "Reply"), options: [.authenticationRequired],
            textInputButtonTitle: String(localized: "Send"), textInputPlaceholder: String(localized: "Message")
        )
        let read = UNNotificationAction(identifier: Self.readAction, title: String(localized: "Mark as read"),
                                        options: [.authenticationRequired])
        center.setNotificationCategories([
            UNNotificationCategory(identifier: Self.messageCategory, actions: [reply, read], intentIdentifiers: [], options: []),
        ])
    }

    /// Asks once (after the welcome screens).
    func requestAuthorization() {
        UNUserNotificationCenter.current().requestAuthorization(options: [.alert, .sound, .badge]) { _, _ in }
    }

    /// Messages, reactions, invitations, MeshCore and channel posts; `actions`: Reply and Mark
    /// as read. With the app lock on, they say only "New message": no sender, no group, no
    /// text on the lock screen, and nothing to answer it with.
    func message(_ chat: ChatID, title: String, body: String, actions: Bool = true) {
        if AppLock.hidesNotifications {
            post(chat, title: "lichen", body: String(localized: "New message"), sos: false, actions: false)
        } else {
            post(chat, title: title, body: body, sos: false, actions: actions)
        }
    }

    /// A message request (ADR-0046): it opens no conversation (it has none yet) and waits in
    /// Messages. With the app lock on, "New message" only, as for messages.
    func request(title: String, body: String) {
        let c = UNMutableNotificationContent()
        let hidden = AppLock.hidesNotifications
        c.title = hidden ? "lichen" : title
        c.body = hidden ? String(localized: "New message") : body
        c.sound = .default
        c.threadIdentifier = "requests"
        let r = UNNotificationRequest(identifier: UUID().uuidString, content: c, trigger: nil)
        UNUserNotificationCenter.current().add(r) { error in
            if let error { NSLog("lichen: notification not posted: \(error.localizedDescription)") }
        }
    }

    /// An answer typed in a notification could not be sent: said in the same place.
    func notSent(_ chat: ChatID, reason: String) {
        if AppLock.hidesNotifications {
            post(chat, title: "lichen", body: String(localized: "Your answer was not sent."), sos: false, actions: false)
        } else {
            post(chat, title: String(localized: "Not sent"), body: reason, sos: false, actions: false)
        }
    }

    /// An SOS keeps its sender and content with the app lock on too: safety comes first.
    func sos(_ chat: ChatID, title: String, body: String) {
        post(chat, title: title, body: body, sos: true, actions: false)
    }

    private func post(_ chat: ChatID, title: String, body: String, sos: Bool, actions: Bool) {
        let c = UNMutableNotificationContent()
        c.title = title
        c.body = body
        if actions { c.categoryIdentifier = Self.messageCategory }
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

    /// The conversation a notification names: a known kind with an id of its length (node
    /// keys 32 bytes, groups 16, channels 8, rooms 32), or nothing.
    private nonisolated static func chat(from info: [AnyHashable: Any]) -> ChatID? {
        guard let kind = info["kind"] as? String, let hex = info["id"] as? String, hex.count <= 64,
              let id = Data(hex: hex) else { return nil }
        return switch (kind, id.count) {
        case ("c", 32): .contact(key: id)
        case ("g", 16): .group(gid: id)
        case ("m", 32): .mesh(key: id)
        case ("h", 8): .channel(id: id)
        case ("r", 32): .room(key: id)
        default: nil
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
            #if DEBUG
            // -noBanners YES (UI tests): no banner over the app, which a test's taps would wait
            // behind; still in the notification list.
            if UserDefaults.standard.bool(forKey: "noBanners") {
                completionHandler(visible ? [] : [.list])
                return
            }
            #endif
            completionHandler(visible ? [] : [.banner, .list, .sound])
        }
    }

    nonisolated func userNotificationCenter(
        _ center: UNUserNotificationCenter, didReceive response: UNNotificationResponse,
        withCompletionHandler completionHandler: @escaping () -> Void
    ) {
        let chat = Self.chat(from: response.notification.request.content.userInfo)
        let action = response.actionIdentifier
        let typed = (response as? UNTextInputNotificationResponse)?.userText
        DispatchQueue.main.async {
            if let chat {
                MainActor.assumeIsolated {
                    switch action {
                    case Self.replyAction:
                        let text = (typed ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
                        if !text.isEmpty, text.utf8.count <= Self.replyMax { self.onReply?(chat, text) }
                    case Self.readAction:
                        self.onMarkRead?(chat)
                    case UNNotificationDefaultActionIdentifier:
                        self.onOpen?(chat)
                    default:
                        break
                    }
                }
            }
            completionHandler()
        }
    }
}
