import Foundation
import CryptoKit

// Client-held KEK protocol (Option B) — iOS implementation.
// Mirrors web/src/lib/clientCrypto.ts and backend mig 0049.
//
// The user's plaintext password NEVER leaves this device. Instead we
// run Argon2id twice — domain-separated by a constant prefix — to
// produce:
//
//   auth_hash = Argon2id("kway-auth-v1::" || password, kek_salt)
//   user_kek  = Argon2id("kway-kek-v1::"  || password, kek_salt)
//
// Both are 32 raw bytes (base64 over the wire). Salt + Argon2 params
// come from GET /auth/kek-params.
//
// ── Xcode setup ────────────────────────────────────────────────────
//
// This file expects the `Argon2Swift` Swift Package
//   https://github.com/tmthecoder/Argon2Swift
// to be added to the project (File → Add Package Dependencies →
// paste the URL).  Argon2Swift wraps the reference argon2 C library
// in a clean Swift API and is the standard choice for iOS apps that
// need Argon2id (CryptoKit only exposes PBKDF2, which won't match
// what the server stored).
//
// If you're reading this in a checkout that hasn't been opened in
// Xcode yet, the `import Argon2Swift` line below will not resolve
// until the package is added.

import Argon2Swift

// MARK: - Wire types

struct KekParams: Decodable {
    let kek_salt: String          // base64 of 32-byte salt
    let argon2_m_cost: Int        // KiB
    let argon2_t_cost: Int
    let argon2_p_cost: Int
    let argon2_output_len: Int
    let kek_domain: String
    let auth_domain: String
}

// MARK: - Helpers

private func b64Encode(_ data: Data) -> String { data.base64EncodedString() }
private func b64Decode(_ s: String) -> Data? { Data(base64Encoded: s.trimmingCharacters(in: .whitespaces)) }

enum ClientCryptoError: LocalizedError {
    case badParams(String)
    case argon2Failed(String)
    var errorDescription: String? {
        switch self {
        case .badParams(let m): return "kek-params error: \(m)"
        case .argon2Failed(let m): return "Argon2id failed: \(m)"
        }
    }
}

// MARK: - Derivation

/// Produce a 32-byte Argon2id output for the given domain + password
/// using the server-published Argon2 parameters.
private func derive(domain: String, password: String, params: KekParams) throws -> Data {
    guard let saltBytes = b64Decode(params.kek_salt) else {
        throw ClientCryptoError.badParams("kek_salt is not valid base64")
    }
    // Concatenate domain + password as the Argon2 "password" input
    // (matches `derive_user_kek` in backend/src/security/vault_crypto.rs).
    var input = Data(domain.utf8)
    input.append(Data(password.utf8))

    let s2 = Salt.newSalt(bytes: saltBytes)
    let result = try Argon2Swift.hashPasswordBytes(
        password: input,
        salt: s2,
        iterations: Int32(params.argon2_t_cost),
        memory: Int32(params.argon2_m_cost),
        parallelism: Int32(params.argon2_p_cost),
        length: Int32(params.argon2_output_len),
        type: Argon2Type.id,
        version: Argon2Version.V13
    )
    return result.hashData()
}

struct DerivedSecrets {
    let authHashB64: String
    let userKekB64: String
    /// Raw KEK bytes for the in-memory holder. Caller is responsible
    /// for zeroing this when the session ends.
    let userKekBytes: Data
}

func deriveAuthAndKek(password: String, params: KekParams) throws -> DerivedSecrets {
    let auth = try derive(domain: params.auth_domain, password: password, params: params)
    let kek  = try derive(domain: params.kek_domain,  password: password, params: params)
    return DerivedSecrets(
        authHashB64: b64Encode(auth),
        userKekB64: b64Encode(kek),
        userKekBytes: kek
    )
}

/// Generate a fresh 32-byte salt for new account registration (base64).
func randomKekSalt() -> String {
    var bytes = [UInt8](repeating: 0, count: 32)
    _ = SecRandomCopyBytes(kSecRandomDefault, bytes.count, &bytes)
    return b64Encode(Data(bytes))
}

// MARK: - In-memory KEK holder (UX-only, see web counterpart)

actor UserKekHolder {
    static let shared = UserKekHolder()
    private var bytes: Data?

    func remember(_ data: Data) { bytes = data }
    func current() -> Data? { bytes }
    func forget() {
        if var b = bytes {
            b.withUnsafeMutableBytes { ptr in
                if let base = ptr.baseAddress { _ = memset_s(base, b.count, 0, b.count) }
            }
            bytes = nil
        }
    }
}
