import Foundation
import Security

/// What reading the identity seed found. Only `absent` may lead to a new identity: a Keychain
/// that cannot be read now, or holds something else, must never be replaced silently.
enum IdentityRead: Equatable {
    case found(Data)
    /// Nothing stored: a first launch.
    case absent
    /// The Keychain could not be read now (before the first unlock, interaction not allowed…).
    case unavailable(OSStatus)
    /// Something is stored, but it is not a 32-byte seed.
    case corrupt
}

/// Why the identity could not be saved.
enum IdentityWriteError: Error {
    case write(OSStatus)
    /// Saved, but reading it back did not give the same seed.
    case verify
    case random
}

/// Secrets in the Keychain, this device only: the phone identity's seed (ADR-0013).
enum Keychain {
    private static let identityAccount = "phone-identity"
    private static let service = "com.tduhamel.lichen.identity"

    private static var base: [String: Any] {
        [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: identityAccount,
        ]
    }

    static func readIdentity() -> IdentityRead {
        var query = base
        query[kSecReturnData as String] = true
        var item: CFTypeRef?
        let status = SecItemCopyMatching(query as CFDictionary, &item)
        switch status {
        case errSecSuccess:
            guard let seed = item as? Data, seed.count == 32 else { return .corrupt }
            return .found(seed)
        case errSecItemNotFound:
            return .absent
        default:
            #if DEBUG
            print("lichen: identity Keychain read failed, status \(status)")
            #endif
            return .unavailable(status)
        }
    }

    /// Draws and saves a new seed: only after `readIdentity()` found none. An item that
    /// appeared meanwhile is not overwritten (the add fails).
    static func createIdentitySeed() throws -> Data {
        let seed = try secureRandom(32)
        var add = base
        add[kSecValueData as String] = seed
        add[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
        let status = SecItemAdd(add as CFDictionary, nil)
        guard status == errSecSuccess else { throw IdentityWriteError.write(status) }
        try verify(seed)
        return seed
    }

    /// Replaces the seed: an identity restored from its 24 words, or a new one the user chose
    /// deliberately. Updated in place, added only when absent, then read back.
    static func replaceIdentitySeed(_ seed: Data) throws {
        let attributes: [String: Any] = [
            kSecValueData as String: seed,
            kSecAttrAccessible as String: kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly,
        ]
        var status = SecItemUpdate(base as CFDictionary, attributes as CFDictionary)
        if status == errSecItemNotFound {
            var add = base
            add.merge(attributes) { _, new in new }
            status = SecItemAdd(add as CFDictionary, nil)
        }
        guard status == errSecSuccess else { throw IdentityWriteError.write(status) }
        try verify(seed)
    }

    private static func verify(_ seed: Data) throws {
        guard case let .found(back) = readIdentity(), back == seed else { throw IdentityWriteError.verify }
    }

    static func secureRandom(_ count: Int) throws -> Data {
        var bytes = [UInt8](repeating: 0, count: count)
        guard SecRandomCopyBytes(kSecRandomDefault, bytes.count, &bytes) == errSecSuccess else {
            throw IdentityWriteError.random
        }
        return Data(bytes)
    }

    // MARK: the database key (ADR-0036)

    private static let databaseService = "com.tduhamel.lichen.database"

    private static var databaseBase: [String: Any] {
        [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: databaseService,
            kSecAttrAccount as String: "database-key",
        ]
    }

    /// What reading the database key found (ADR-0036).
    enum DatabaseKeyRead: Equatable {
        case found(Data)
        /// None stored yet.
        case absent
        /// The Keychain cannot give it now (before the first unlock).
        case unavailable
        /// Something is stored, but not a 32-byte key.
        case lost
    }

    /// The key of the message databases (SQLCipher, ADR-0036): 32 random bytes drawn once,
    /// kept in the Keychain for this device only, readable after the first unlock.
    static func readDatabaseKey() -> DatabaseKeyRead {
        var query = databaseBase
        query[kSecReturnData as String] = true
        var item: CFTypeRef?
        switch SecItemCopyMatching(query as CFDictionary, &item) {
        case errSecSuccess:
            guard let key = item as? Data, key.count == 32 else { return .lost }
            return .found(key)
        case errSecItemNotFound:
            return .absent
        default:
            return .unavailable
        }
    }

    /// Draws and saves the database key: only after `readDatabaseKey()` found none (or a lost
    /// one, which it replaces) and no database waits for an older one.
    static func createDatabaseKey() throws -> Data {
        if readDatabaseKey() == .lost { SecItemDelete(databaseBase as CFDictionary) }
        let key = try secureRandom(32)
        var add = databaseBase
        add[kSecValueData as String] = key
        add[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
        let status = SecItemAdd(add as CFDictionary, nil)
        guard status == errSecSuccess else { throw IdentityWriteError.write(status) }
        // Read back: what the next start will find.
        guard readDatabaseKey() == .found(key) else { throw IdentityWriteError.verify }
        return key
    }

    #if DEBUG
    /// Removes the identity and the database key (debug `freshInstall`, UI tests).
    static func debugErase() {
        SecItemDelete(base as CFDictionary)
        SecItemDelete(databaseBase as CFDictionary)
    }
    #endif

    /// Seeds for the core's random generator and for throwaway identities (demo).
    static func randomSeed() -> Data {
        // SecRandomCopyBytes does not fail on a working system; an all-zero seed would be worse
        // than stopping.
        guard let seed = try? secureRandom(32) else { fatalError("no system randomness") }
        return seed
    }
}
