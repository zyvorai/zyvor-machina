import Foundation
import CryptoKit
import Security

public enum TrustDecision: Sendable { case trusted, firstUse(fingerprint: String), mismatch(expected: String, got: String) }

/// Trust-on-first-use pinning for self-signed daemon certificates (SHA-256 of the leaf).
public final class PinnedTLSDelegate: NSObject, URLSessionDelegate, @unchecked Sendable {
    private let lock = NSLock()
    private var pinned: String?
    /// Called on first sight of an unpinned cert; return true to trust and pin.
    private let confirmFirstUse: @Sendable (String) async -> Bool
    private let persist: @Sendable (String) -> Void

    public init(pinned: String?, confirmFirstUse: @escaping @Sendable (String) async -> Bool,
                persist: @escaping @Sendable (String) -> Void) {
        self.pinned = pinned; self.confirmFirstUse = confirmFirstUse; self.persist = persist
    }

    public static func fingerprint(of trust: SecTrust) -> String? {
        guard let chain = SecTrustCopyCertificateChain(trust) as? [SecCertificate],
              let leaf = chain.first else { return nil }
        let der = SecCertificateCopyData(leaf) as Data
        return SHA256.hash(data: der).map { String(format: "%02x", $0) }.joined()
    }

    public func urlSession(_ session: URLSession, didReceive challenge: URLAuthenticationChallenge) async
        -> (URLSession.AuthChallengeDisposition, URLCredential?) {
        guard challenge.protectionSpace.authenticationMethod == NSURLAuthenticationMethodServerTrust,
              let trust = challenge.protectionSpace.serverTrust,
              let fp = Self.fingerprint(of: trust) else { return (.performDefaultHandling, nil) }
        // Publicly trusted certs need no pin.
        if SecTrustEvaluateWithError(trust, nil) { return (.useCredential, URLCredential(trust: trust)) }
        let current = lock.withLock { pinned }
        if let current {
            return current == fp ? (.useCredential, URLCredential(trust: trust)) : (.cancelAuthenticationChallenge, nil)
        }
        if await confirmFirstUse(fp) {
            lock.withLock { pinned = fp }
            persist(fp)
            return (.useCredential, URLCredential(trust: trust))
        }
        return (.cancelAuthenticationChallenge, nil)
    }
}
