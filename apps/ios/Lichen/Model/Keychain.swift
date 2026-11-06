import Foundation
import Security

/// Secrets in the Keychain, this device only: the phone identity's seed (ADR-0013).
enum Keychain {
    private static let identityAccount = "phone-identity"

    /// Our identity seed: drawn on first launch and kept, never synced.
    static func identitySeed() -> Data {
        if let seed = load(node: identityAccount), seed.count == 32 { return seed }
        let seed = randomSeed()
        save(seed, node: identityAccount)
        return seed
    }

    /// Replaces the identity seed (restored from its 24 words).
    static func setIdentitySeed(_ seed: Data) {
        save(seed, node: identityAccount)
    }

    private static let service = "com.tduhamel.lichen.identity"

    static func load(node: String) -> Data? {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: node,
            kSecReturnData as String: true,
        ]
        var item: CFTypeRef?
        guard SecItemCopyMatching(query as CFDictionary, &item) == errSecSuccess else { return nil }
        return item as? Data
    }

    static func save(_ key: Data, node: String) {
        let base: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: node,
        ]
        SecItemDelete(base as CFDictionary)
        var add = base
        add[kSecValueData as String] = key
        add[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
        SecItemAdd(add as CFDictionary, nil)
    }

    static func randomSeed() -> Data {
        var bytes = [UInt8](repeating: 0, count: 32)
        _ = SecRandomCopyBytes(kSecRandomDefault, bytes.count, &bytes)
        return Data(bytes)
    }
}
