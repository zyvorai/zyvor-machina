import Foundation
import CryptoKit

/// Pure logic for installing an update: checksum, signer comparison, download policy and the app swap.
/// The Process / mount / relaunch parts live in the app target (`Machina/SelfUpdate.swift`).
public enum SelfUpdate {
    public enum Failure: Error, LocalizedError, Equatable, Sendable {
        case insecureURL, download(String), hashMismatch(expected: String, actual: String)
        case notNewer(current: String, offered: String), wrongApp(String), signer(String), gatekeeper(String)
        case install(String)
        public var errorDescription: String? {
            switch self {
            case .insecureURL: return "The update is not served over HTTPS, so it was not downloaded."
            case .download(let m): return "Download failed: \(m)"
            case .hashMismatch(let e, let a): return "The download's SHA-256 (\(a.prefix(12))…) does not match the release notes (\(e.prefix(12))…). It was deleted and nothing was installed."
            case .notNewer(let c, let o): return "The downloaded app is version \(o), not newer than \(c)."
            case .wrongApp(let m): return "The downloaded app is not Machina: \(m)"
            case .signer(let m): return "Signature check failed: \(m)"
            case .gatekeeper(let m): return "Gatekeeper rejected the app: \(m)"
            case .install(let m): return "Install failed: \(m)"
            }
        }
    }

    // MARK: checksum

    public static func sha256Hex(of url: URL) throws -> String {
        let h = try FileHandle(forReadingFrom: url); defer { try? h.close() }
        var hasher = SHA256()
        while let chunk = try h.read(upToCount: 1 << 20), !chunk.isEmpty { hasher.update(data: chunk) }
        return hasher.finalize().map { String(format: "%02x", $0) }.joined()
    }

    /// Case-insensitive, whitespace-tolerant comparison of two hex digests.
    public static func hashesMatch(_ a: String, _ b: String) -> Bool {
        let x = a.trimmingCharacters(in: .whitespacesAndNewlines).lowercased(), y = b.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        return x.count == 64 && x == y
    }

    private static func hexTokens(_ line: String) -> [String] {
        line.split(whereSeparator: { !($0.isHexDigit) }).map(String.init).filter { $0.count == 64 }.map { $0.lowercased() }
    }

    /// The DMG's SHA-256 from the release notes. A line naming the asset wins; otherwise a line mentioning "sha256" / "sha-256";
    /// nil when there is none or the notes disagree with themselves (never guess).
    public static func expectedSHA256(notes: String?, assetName: String?) -> String? {
        guard let notes else { return nil }
        let lines = notes.split(whereSeparator: \.isNewline).map(String.init)
        func pick(_ ls: [String]) -> String? {
            let all = Set(ls.flatMap(hexTokens))
            return all.count == 1 ? all.first : nil
        }
        if let name = assetName?.lowercased(), !name.isEmpty {
            let named = lines.filter { $0.lowercased().contains(name) && !hexTokens($0).isEmpty }
            if !named.isEmpty { return pick(named) }
        }
        let labelled = lines.filter { let l = $0.lowercased(); return (l.contains("sha256") || l.contains("sha-256")) && !hexTokens($0).isEmpty }
        return pick(labelled)
    }

    // MARK: signer

    /// `TeamIdentifier=` from `codesign -dv` output (stderr). nil for ad-hoc / unsigned apps (`not set`).
    public static func teamID(fromCodesign output: String) -> String? {
        for line in output.split(whereSeparator: \.isNewline) where line.hasPrefix("TeamIdentifier=") {
            let v = line.dropFirst("TeamIdentifier=".count).trimmingCharacters(in: .whitespaces)
            return v.isEmpty || v == "not set" ? nil : v
        }
        return nil
    }

    /// Both must be real team ids and equal. An unsigned running app can never authorise an update.
    public static func sameTeam(running: String?, candidate: String?) -> Bool {
        guard let r = running, let c = candidate, !r.isEmpty else { return false }
        return r == c
    }

    /// The `Authority=Developer ID Application` leaf must be present (Developer ID, not Apple Development or ad hoc).
    public static func isDeveloperID(codesign output: String) -> Bool {
        output.split(whereSeparator: \.isNewline).contains { $0.hasPrefix("Authority=Developer ID Application:") }
    }

    // MARK: policy

    public static func isNewer(offered: String, current: String) -> Bool {
        guard let o = AppVersion(offered), let c = AppVersion(current) else { return false }
        return o > c
    }

    /// Downloads must be HTTPS and from a GitHub host.
    public static func downloadAllowed(_ url: URL) -> Bool {
        guard url.scheme == "https", let h = url.host?.lowercased() else { return false }
        return h == "github.com" || h.hasSuffix(".github.com") || h.hasSuffix(".githubusercontent.com")
    }

    /// Session for the download: no cookies, no credential storage, no cache.
    public static func downloadSession() -> URLSession {
        let c = URLSessionConfiguration.ephemeral
        c.httpCookieStorage = nil; c.httpShouldSetCookies = false
        c.urlCredentialStorage = nil; c.urlCache = nil; c.requestCachePolicy = .reloadIgnoringLocalCacheData
        c.timeoutIntervalForRequest = 30; c.timeoutIntervalForResource = 1800
        c.httpAdditionalHeaders = [:]
        return URLSession(configuration: c)
    }

    // MARK: swap

    /// Replaces `destination` with `newApp`. The new copy is placed next to the destination first, the old app is moved
    /// aside, and on any failure the old one is put back. Nothing is touched if `newApp` is missing.
    public static func replaceApp(at destination: URL, with newApp: URL, fm: FileManager = .default) throws {
        guard fm.fileExists(atPath: newApp.path) else { throw Failure.install("the new app is missing") }
        let dir = destination.deletingLastPathComponent()
        let tag = UUID().uuidString.prefix(8)
        let staged = dir.appendingPathComponent(".\(destination.lastPathComponent).new-\(tag)")
        let backup = dir.appendingPathComponent(".\(destination.lastPathComponent).old-\(tag)")
        do { try fm.copyItem(at: newApp, to: staged) } catch { throw Failure.install("could not copy into \(dir.path): \(error.localizedDescription)") }
        let hadOld = fm.fileExists(atPath: destination.path)
        do {
            if hadOld { try fm.moveItem(at: destination, to: backup) }
            do { try fm.moveItem(at: staged, to: destination) }
            catch {
                if hadOld { try? fm.moveItem(at: backup, to: destination) }
                throw error
            }
        } catch {
            try? fm.removeItem(at: staged)
            throw Failure.install(error.localizedDescription)
        }
        if hadOld { try? fm.removeItem(at: backup) }
    }
}
