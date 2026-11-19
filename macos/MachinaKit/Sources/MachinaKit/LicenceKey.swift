import Foundation
import CryptoKit

/// What a licence key says about this Mac's entitlement. Display only: nothing in Machina is disabled in any state.
public enum LicenceStatus: Equatable, Sendable {
    case none
    case valid(edition: String, licensee: String, email: String, macs: Int, daysLeft: Int, id: String)
    case invalid(reason: String)

    public var isActive: Bool { if case .valid = self { return true }; return false }
}

/// Offline verifier for `ZYV1.<b64url payload>.<b64url Ed25519 signature>` keys.
/// No network, no hardware binding: the signature must verify, the product must match, and now must be before `expires`.
/// Only the public key lives here; keys are issued elsewhere.
public enum LicenceKey {
    /// Ed25519 public key (base64url, raw 32 bytes) for product "machina".
    public static let publicKey = "OWvIiwyM227lstCP3hqPI9Mxqi-wbcfiMWRSHNqf2Ec"
    public static let product = "machina"
    public static let prefix = "ZYV1"

    private struct Payload: Decodable {
        let v: Int; let product: String; let edition: String; let id: String
        let licensee: String; let email: String; let macs: Int; let issued: Int; let expires: Int
    }

    public static func decode(base64url s: String) -> Data? {
        guard !s.isEmpty, s.unicodeScalars.allSatisfy({ ($0.value < 128) && (CharacterSet.alphanumerics.contains($0) || $0 == "-" || $0 == "_") }) else { return nil }
        var t = s.replacingOccurrences(of: "-", with: "+").replacingOccurrences(of: "_", with: "/")
        t += String(repeating: "=", count: (4 - t.count % 4) % 4)
        return Data(base64Encoded: t)
    }

    /// What a genuine key says, without looking at the clock: the signature and product must verify, but an expired key still
    /// reports its expiry date so the UI can say "Licence expired on ...". Nil when malformed, altered or for another product.
    public struct Claims: Equatable, Sendable {
        public let edition: String, licensee: String, email: String, id: String
        public let macs: Int
        public let issued: Date, expires: Date
    }

    public static func claims(_ key: String, publicKey: String = LicenceKey.publicKey, product: String = LicenceKey.product) -> Claims? {
        let parts = key.trimmingCharacters(in: .whitespacesAndNewlines).split(separator: ".", omittingEmptySubsequences: false).map(String.init)
        guard parts.count == 3, parts[0] == prefix, let body = decode(base64url: parts[1]), let sig = decode(base64url: parts[2]),
              let pubData = decode(base64url: publicKey), let pub = try? Curve25519.Signing.PublicKey(rawRepresentation: pubData),
              pub.isValidSignature(sig, for: body), let p = try? JSONDecoder().decode(Payload.self, from: body),
              p.v == 1, p.product == product, p.edition == "trial" || p.edition == "pro" else { return nil }
        return Claims(edition: p.edition, licensee: p.licensee, email: p.email, id: p.id, macs: p.macs,
                      issued: Date(timeIntervalSince1970: Double(p.issued)), expires: Date(timeIntervalSince1970: Double(p.expires)))
    }

    public static func verify(_ key: String, publicKey: String = LicenceKey.publicKey, now: Date = Date(), product: String = LicenceKey.product) -> LicenceStatus {
        let parts = key.trimmingCharacters(in: .whitespacesAndNewlines).split(separator: ".", omittingEmptySubsequences: false).map(String.init)
        guard parts.count == 3, parts[0] == prefix else { return .invalid(reason: "bad format: expected ZYV1.<payload>.<signature>") }
        guard let body = decode(base64url: parts[1]), let sig = decode(base64url: parts[2]) else { return .invalid(reason: "bad format: payload or signature is not base64url") }
        guard let pubData = decode(base64url: publicKey), let pub = try? Curve25519.Signing.PublicKey(rawRepresentation: pubData),
              pub.isValidSignature(sig, for: body) else { return .invalid(reason: "signature does not match (key was altered or is not from Zyvor)") }
        guard let p = try? JSONDecoder().decode(Payload.self, from: body) else { return .invalid(reason: "bad format: missing or invalid fields") }
        guard p.v == 1 else { return .invalid(reason: "unsupported licence version") }
        guard p.product == product else { return .invalid(reason: "wrong product: key is for \"\(p.product)\", not \"\(product)\"") }
        guard p.edition == "trial" || p.edition == "pro" else { return .invalid(reason: "bad format: unknown edition") }
        let t = now.timeIntervalSince1970
        guard t < Double(p.expires) else { return .invalid(reason: "expired \(Int((t - Double(p.expires)) / 86400)) day(s) ago") }
        let left = Int(((Double(p.expires) - t) / 86400).rounded(.up))
        return .valid(edition: p.edition, licensee: p.licensee, email: p.email, macs: p.macs, daysLeft: left, id: p.id)
    }
}
