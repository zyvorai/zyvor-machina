import Foundation

/// Dotted numeric version, tolerant of a leading "v" and a "-suffix" (`v0.3.2-rc1` -> 0.3.2).
public struct AppVersion: Comparable, Equatable, Sendable, CustomStringConvertible {
    public let parts: [Int]
    public init?(_ s: String) {
        var t = s.trimmingCharacters(in: .whitespaces)
        if t.hasPrefix("v") || t.hasPrefix("V") { t.removeFirst() }
        if let i = t.firstIndex(where: { $0 == "-" || $0 == "+" }) { t = String(t[..<i]) }
        let nums = t.split(separator: ".", omittingEmptySubsequences: false).map { Int($0) }
        guard !nums.isEmpty, !nums.contains(where: { $0 == nil }) else { return nil }
        parts = nums.compactMap { $0 }
    }
    public var description: String { parts.map(String.init).joined(separator: ".") }
    public static func < (a: AppVersion, b: AppVersion) -> Bool {
        for i in 0..<max(a.parts.count, b.parts.count) {
            let x = i < a.parts.count ? a.parts[i] : 0, y = i < b.parts.count ? b.parts[i] : 0
            if x != y { return x < y }
        }
        return false
    }
    public static func == (a: AppVersion, b: AppVersion) -> Bool { !(a < b) && !(b < a) }
}

public struct ReleaseInfo: Equatable, Sendable {
    public var version: String
    public var pageURL: URL
    public var dmgURL: URL?
    /// The release body (markdown); `SelfUpdate.expectedSHA256` reads the DMG's checksum from it.
    public var notes: String? = nil
    public var dmgName: String? = nil
}

public enum UpdateStatus: Equatable, Sendable {
    case upToDate(String)
    case available(ReleaseInfo)
    case failed(String)
}

public enum UpdateChecker {
    /// The repository's `v*` releases belong to the Machina platform; the Mac app publishes as `macos-v<version>`.
    public static let tagPrefix = "macos-v"
    public static let latestURL = URL(string: "https://api.github.com/repos/zyvorai/zyvor-machina/releases?per_page=50")!

    /// Picks the highest `macos-v*` release from GitHub's release list. Drafts and prereleases are ignored.
    public static func parseList(_ data: Data) -> ReleaseInfo? {
        guard let arr = try? JSONSerialization.jsonObject(with: data) as? [[String: Any]] else { return nil }
        let found: [(AppVersion, ReleaseInfo)] = arr.compactMap { o in
            guard let tag = o["tag_name"] as? String, tag.hasPrefix(tagPrefix),
                  let v = AppVersion(String(tag.dropFirst(tagPrefix.count))) else { return nil }
            var o = o; o["tag_name"] = String(tag.dropFirst(tagPrefix.count))
            guard let d = try? JSONSerialization.data(withJSONObject: o), let info = parse(d) else { return nil }
            return (v, info)
        }
        return found.max { $0.0 < $1.0 }?.1
    }

    /// Parse GitHub's "latest release" JSON. Drafts and prereleases are ignored.
    public static func parse(_ data: Data) -> ReleaseInfo? {
        guard let o = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              (o["draft"] as? Bool) != true, (o["prerelease"] as? Bool) != true,
              let tag = o["tag_name"] as? String, AppVersion(tag) != nil,
              let page = (o["html_url"] as? String).flatMap(URL.init(string:)) else { return nil }
        let assets = o["assets"] as? [[String: Any]] ?? []
        let dmgAsset = assets.first { ($0["name"] as? String)?.lowercased().hasSuffix(".dmg") == true }
        let dmg = dmgAsset.flatMap { ($0["browser_download_url"] as? String).flatMap(URL.init(string:)) }
        return ReleaseInfo(version: tag.hasPrefix("v") ? String(tag.dropFirst()) : tag, pageURL: page, dmgURL: dmg,
                           notes: o["body"] as? String, dmgName: dmgAsset?["name"] as? String)
    }

    public static func evaluate(current: String, release: ReleaseInfo) -> UpdateStatus {
        guard let c = AppVersion(current), let r = AppVersion(release.version) else { return .failed("Unrecognised version") }
        return r > c ? .available(release) : .upToDate(current)
    }

    public static func check(current: String, session: URLSession = .shared) async -> UpdateStatus {
        var req = URLRequest(url: latestURL, timeoutInterval: 15)
        req.setValue("application/vnd.github+json", forHTTPHeaderField: "Accept")
        do {
            let (data, resp) = try await session.data(for: req)
            let code = (resp as? HTTPURLResponse)?.statusCode ?? 0
            guard code == 200 else { return .failed("GitHub answered HTTP \(code)") }
            guard let rel = parseList(data) else { return .failed("No Mac app release found") }
            return evaluate(current: current, release: rel)
        } catch { return .failed(error.localizedDescription) }
    }
}
