import SwiftUI
import AppKit
import MachinaKit

/// Observable state for the progress window.
@MainActor @Observable final class UpdateProgress {
    var message = ""
    var fraction: Double?       // nil = indeterminate
    var error: String?
    var finished = false
}

private struct UpdateProgressView: View {
    let p: UpdateProgress
    let close: () -> Void
    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Updating Machina").font(.headline)
            if let e = p.error {
                Label(e, systemImage: "xmark.octagon.fill").foregroundStyle(.red).textSelection(.enabled)
                Text("Nothing was installed. The current app is unchanged.").font(.callout).foregroundStyle(.secondary)
                HStack { Spacer(); Button("Close", action: close).keyboardShortcut(.defaultAction) }
            } else {
                if let f = p.fraction { ProgressView(value: f) } else { ProgressView().progressViewStyle(.linear) }
                Text(p.message).font(.callout).foregroundStyle(.secondary)
            }
        }.padding(20).frame(width: 420)
    }
}

/// Download, verify, and install a release. Every check must pass; any failure leaves the installed app untouched.
@MainActor enum SelfUpdater {
    struct Verified { var stagedApp: URL; var workDir: URL; var version: String; var hashChecked: Bool }

    private static var window: NSWindow?

    static func start(_ rel: ReleaseInfo) async {
        guard let dmg = rel.dmgURL else { NSWorkspace.shared.open(rel.pageURL); return }
        let progress = UpdateProgress()
        showWindow(progress)
        let work = FileManager.default.temporaryDirectory.appendingPathComponent("machina-update-\(UUID().uuidString)")
        do {
            try FileManager.default.createDirectory(at: work, withIntermediateDirectories: true)
            let v = try await prepare(rel, dmg: dmg, work: work, progress: progress)
            window?.close(); window = nil
            guard confirm(v, current: Updates.currentVersion) else { cleanup(work); return }
            let dest = destination()
            try SelfUpdate.replaceApp(at: dest, with: v.stagedApp)
            cleanup(work)
            relaunch(dest)
        } catch {
            cleanup(work)
            progress.error = error.localizedDescription
            if window == nil { showWindow(progress) }
        }
    }

    // MARK: steps

    static func prepare(_ rel: ReleaseInfo, dmg: URL, work: URL, progress: UpdateProgress) async throws -> Verified {
        guard SelfUpdate.downloadAllowed(dmg) else { throw SelfUpdate.Failure.insecureURL }
        progress.message = "Downloading Machina \(rel.version)…"
        let file = work.appendingPathComponent("Machina.dmg")
        try await download(dmg, to: file, progress: progress)

        progress.fraction = nil; progress.message = "Verifying checksum…"
        var hashChecked = false
        if let expected = SelfUpdate.expectedSHA256(notes: rel.notes, assetName: rel.dmgName) {
            let actual = try SelfUpdate.sha256Hex(of: file)
            guard SelfUpdate.hashesMatch(expected, actual) else {
                try? FileManager.default.removeItem(at: file)
                throw SelfUpdate.Failure.hashMismatch(expected: expected, actual: actual)
            }
            hashChecked = true
        }

        progress.message = "Opening the disk image…"
        let mount = work.appendingPathComponent("mnt")
        try FileManager.default.createDirectory(at: mount, withIntermediateDirectories: true)
        let attach = await run("/usr/bin/hdiutil", ["attach", "-nobrowse", "-readonly", "-noverify", "-noautoopen", "-mountpoint", mount.path, file.path])
        guard attach.status == 0 else { throw SelfUpdate.Failure.install("could not mount the disk image: \(attach.output.suffix(200))") }
        defer { Task { _ = await run("/usr/bin/hdiutil", ["detach", "-force", mount.path]) } }

        let apps = (try? FileManager.default.contentsOfDirectory(at: mount, includingPropertiesForKeys: nil))?.filter { $0.pathExtension == "app" } ?? []
        guard apps.count == 1, let inDMG = apps.first else { throw SelfUpdate.Failure.wrongApp("the disk image holds \(apps.count) apps, expected exactly one") }

        // Copy out first and verify the copy, so what is checked is what is installed.
        progress.message = "Copying the app…"
        let staged = work.appendingPathComponent("stage").appendingPathComponent(inDMG.lastPathComponent)
        try FileManager.default.createDirectory(at: staged.deletingLastPathComponent(), withIntermediateDirectories: true)
        let ditto = await run("/usr/bin/ditto", [inDMG.path, staged.path])
        guard ditto.status == 0 else { throw SelfUpdate.Failure.install("copy failed: \(ditto.output.suffix(200))") }

        progress.message = "Checking the signature…"
        let offered = try checkIdentity(staged)
        guard SelfUpdate.isNewer(offered: offered, current: Updates.currentVersion) else {
            throw SelfUpdate.Failure.notNewer(current: Updates.currentVersion, offered: offered)
        }
        let cs = await run("/usr/bin/codesign", ["--verify", "--deep", "--strict", staged.path])
        guard cs.status == 0 else { throw SelfUpdate.Failure.signer("codesign --verify: \(cs.output.suffix(300))") }
        let candidate = await run("/usr/bin/codesign", ["-dvv", staged.path])
        let running = await run("/usr/bin/codesign", ["-dvv", Bundle.main.bundlePath])
        guard SelfUpdate.isDeveloperID(codesign: candidate.output) else { throw SelfUpdate.Failure.signer("the new app is not signed with a Developer ID") }
        guard SelfUpdate.sameTeam(running: SelfUpdate.teamID(fromCodesign: running.output), candidate: SelfUpdate.teamID(fromCodesign: candidate.output)) else {
            throw SelfUpdate.Failure.signer("the new app is signed by a different team than the running app (or the running app is unsigned)")
        }

        progress.message = "Asking Gatekeeper…"
        let spctl = await run("/usr/sbin/spctl", ["--assess", "--type", "execute", "-vv", staged.path])
        guard spctl.status == 0 else { throw SelfUpdate.Failure.gatekeeper(String(spctl.output.suffix(300))) }

        return Verified(stagedApp: staged, workDir: work, version: offered, hashChecked: hashChecked)
    }

    /// Bundle id must match the running app; returns the offered short version.
    private static func checkIdentity(_ app: URL) throws -> String {
        guard let b = Bundle(url: app), let id = b.bundleIdentifier else { throw SelfUpdate.Failure.wrongApp("no bundle identifier") }
        guard id == Bundle.main.bundleIdentifier else { throw SelfUpdate.Failure.wrongApp("bundle id \(id)") }
        guard let v = b.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String else { throw SelfUpdate.Failure.wrongApp("no version") }
        return v
    }

    private static func download(_ url: URL, to file: URL, progress: UpdateProgress) async throws {
        var req = URLRequest(url: url, timeoutInterval: 30)
        req.httpShouldHandleCookies = false
        let session = SelfUpdate.downloadSession()
        let delegate = ProgressDelegate { f in Task { @MainActor in progress.fraction = f } }
        let (tmp, resp) = try await session.download(for: req, delegate: delegate)
        let code = (resp as? HTTPURLResponse)?.statusCode ?? 0
        guard code == 200 else { throw SelfUpdate.Failure.download("HTTP \(code)") }
        // A redirect must stay on an allowed host.
        if let final = resp.url, !SelfUpdate.downloadAllowed(final) { throw SelfUpdate.Failure.insecureURL }
        try FileManager.default.moveItem(at: tmp, to: file)
    }

    private final class ProgressDelegate: NSObject, URLSessionDownloadDelegate, @unchecked Sendable {
        let report: @Sendable (Double) -> Void
        init(_ r: @escaping @Sendable (Double) -> Void) { report = r }
        func urlSession(_ s: URLSession, downloadTask: URLSessionDownloadTask, didFinishDownloadingTo location: URL) {}
        func urlSession(_ s: URLSession, downloadTask: URLSessionDownloadTask, didWriteData _: Int64, totalBytesWritten w: Int64, totalBytesExpectedToWrite t: Int64) {
            if t > 0 { report(Double(w) / Double(t)) }
        }
        func urlSession(_ s: URLSession, task: URLSessionTask, willPerformHTTPRedirection r: HTTPURLResponse, newRequest: URLRequest,
                        completionHandler: @escaping (URLRequest?) -> Void) {
            // Refuse redirects off GitHub / to plain http; never forward credentials (there are none).
            completionHandler(newRequest.url.map(SelfUpdate.downloadAllowed) == true ? newRequest : nil)
        }
    }

    // MARK: UI and process helpers

    private static func confirm(_ v: Verified, current: String) -> Bool {
        let a = NSAlert()
        a.messageText = String(localized: "Install Machina \(v.version)?")
        var info = String(localized: "The download passed the signature and Gatekeeper checks (same Developer ID team as this app). Machina will replace itself and relaunch; you have \(current).")
        if !v.hashChecked { info += "\n\n" + String(localized: "The release notes had no SHA-256, so the checksum could not be compared.") }
        a.informativeText = info
        a.addButton(withTitle: String(localized: "Install and Relaunch"))
        a.addButton(withTitle: String(localized: "Cancel"))
        return a.runModal() == .alertFirstButtonReturn
    }

    /// The running bundle when it is a normal .app, else /Applications/Machina.app.
    private static func destination() -> URL {
        let b = Bundle.main.bundleURL
        return b.pathExtension == "app" ? b : URL(fileURLWithPath: "/Applications/Machina.app")
    }

    private static func relaunch(_ app: URL) {
        let pid = ProcessInfo.processInfo.processIdentifier
        let p = Process()
        p.executableURL = URL(fileURLWithPath: "/bin/sh")
        p.arguments = ["-c", "while kill -0 \"$1\" 2>/dev/null; do sleep 0.3; done; /usr/bin/open \"$2\"", "sh", String(pid), app.path]
        try? p.run()
        NSApp.terminate(nil)
    }

    private static func cleanup(_ dir: URL) { try? FileManager.default.removeItem(at: dir) }

    private static func showWindow(_ p: UpdateProgress) {
        window?.close()
        let w = NSWindow(contentViewController: NSHostingController(rootView: UpdateProgressView(p: p) { window?.close(); window = nil }))
        w.styleMask = [.titled]; w.title = "Machina"; w.center(); w.isReleasedWhenClosed = false
        w.makeKeyAndOrderFront(nil); window = w
    }

    static func run(_ tool: String, _ args: [String]) async -> (status: Int32, output: String) {
        await withCheckedContinuation { cont in
            DispatchQueue.global().async {
                let p = Process(); p.executableURL = URL(fileURLWithPath: tool); p.arguments = args
                let pipe = Pipe(); p.standardOutput = pipe; p.standardError = pipe
                do { try p.run() } catch { cont.resume(returning: (-1, error.localizedDescription)); return }
                let data = pipe.fileHandleForReading.readDataToEndOfFile(); p.waitUntilExit()
                cont.resume(returning: (p.terminationStatus, String(decoding: data, as: UTF8.self)))
            }
        }
    }
}
