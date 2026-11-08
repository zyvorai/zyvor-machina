import SwiftUI
import AppKit
import MachinaKit

struct Transfer: Identifiable {
    enum Kind { case upload, download }
    enum State: Equatable { case running, done, failed(String), cancelled }
    let id = UUID()
    var kind: Kind
    var name: String
    var sent: Int64 = 0
    var total: Int64?
    var state: State = .running
    var fraction: Double? { total.flatMap { $0 > 0 ? min(1, Double(sent) / Double($0)) : nil } }
}

/// ISO uploads (one streaming request each) and server-side downloads (a job, polled). Lives in `AppModel` so a drop
/// anywhere in the window and the ISO page see the same list.
@MainActor @Observable
final class TransferCenter {
    var items: [Transfer] = []
    /// Called when a transfer finishes so the ISO list can reload.
    var onFinished: (@MainActor () -> Void)?
    private var tasks: [UUID: Task<Void, Never>] = [:]

    var hasRunning: Bool { items.contains { $0.state == .running } }

    func upload(_ files: [URL], overwrite: Bool, client: MachinaClient) {
        for file in files {
            let size = (try? FileManager.default.attributesOfItem(atPath: file.path)[.size] as? NSNumber)?.int64Value
            var t = Transfer(kind: .upload, name: file.lastPathComponent); t.total = size
            let id = t.id
            items.insert(t, at: 0)
            tasks[id] = Task { [weak self] in
                do {
                    try await client.uploadIso(file: file, overwrite: overwrite) { sent, total in
                        Task { @MainActor in self?.update(id) { $0.sent = sent; $0.total = total } }
                    }
                    self?.finish(id, .done)
                } catch is CancellationError { self?.finish(id, .cancelled) }
                catch let e as URLError where e.code == .cancelled { self?.finish(id, .cancelled) }
                catch { self?.finish(id, .failed(error.localizedDescription)) }
            }
        }
    }

    func download(url: String, filename: String?, overwrite: Bool, client: MachinaClient) {
        let label = filename.flatMap { $0.isEmpty ? nil : $0 } ?? URL(string: url)?.lastPathComponent ?? url
        let t = Transfer(kind: .download, name: label)
        let id = t.id
        items.insert(t, at: 0)
        tasks[id] = Task { [weak self] in
            do {
                let started = try await client.startIsoDownload(url: url, filename: filename, overwrite: overwrite)
                self?.update(id) { $0.name = started.name }
                try await client.waitForJob(started.jobId) { j in
                    Task { @MainActor in self?.update(id) { $0.sent = j.bytesDone ?? $0.sent; $0.total = j.bytesTotal } }
                }
                self?.finish(id, .done)
            } catch is CancellationError { self?.finish(id, .cancelled) }
            catch { self?.finish(id, .failed(error.localizedDescription)) }
        }
    }

    /// Cancelling an upload aborts the request (the daemon removes its `.part` file). A download keeps running on the
    /// server (there is no job cancel route); only the polling stops.
    func cancel(_ id: UUID) { tasks[id]?.cancel() }

    func clearFinished() { items.removeAll { $0.state != .running } }

    private func update(_ id: UUID, _ change: (inout Transfer) -> Void) {
        guard let i = items.firstIndex(where: { $0.id == id }), items[i].state == .running else { return }
        change(&items[i])
    }

    private func finish(_ id: UUID, _ state: Transfer.State) {
        tasks[id] = nil
        guard let i = items.firstIndex(where: { $0.id == id }) else { return }
        items[i].state = state
        if state == .done, let total = items[i].total { items[i].sent = total }
        onFinished?()
    }
}

extension AppModel {
    /// Window drop: keeps `.iso` files, asks once, then uploads. Everything else is reported, never sent.
    @discardableResult
    func handleDrop(_ urls: [URL]) -> Bool {
        let (ok, skipped) = IsoRules.split(urls)
        guard canWrite else { error = "Upload ISO: your role (\(role ?? "unknown")) cannot make changes"; return false }
        guard !ok.isEmpty else {
            error = "Only .iso files can be uploaded" + (skipped.isEmpty ? "" : " (skipped: \(skipped.prefix(3).joined(separator: ", ")))")
            return false
        }
        confirmUpload(ok, overwrite: false, skipped: skipped)
        return true
    }

    func confirmUpload(_ files: [URL], overwrite: Bool, skipped: [String] = []) {
        let fmt = ByteCountFormatter()
        let list = files.map { f in
            let size = (try? FileManager.default.attributesOfItem(atPath: f.path)[.size] as? NSNumber)?.int64Value ?? 0
            return "\(f.lastPathComponent) (\(fmt.string(fromByteCount: size)))"
        }.joined(separator: "\n")
        var message = "Upload to the hypervisor's ISO directory:\n\(list)\n\(overwrite ? "A file with the same name will be replaced." : "A file with the same name is kept and the upload fails.")"
        if !skipped.isEmpty { message += "\nSkipped (not .iso): \(skipped.prefix(5).joined(separator: ", "))" }
        confirmAction("Upload \(files.count) ISO file\(files.count == 1 ? "" : "s")?", message: message, label: "Upload", destructive: false) { [self] in
            guard canWrite, let client else { error = "Upload ISO: not allowed"; return }
            selection = .page("isos")
            transfers.upload(files, overwrite: overwrite, client: client)
        }
    }

    /// The Upload… button: pick files, choose what to do on a name clash, then confirm.
    func pickISOsToUpload() {
        let panel = NSOpenPanel()
        panel.allowsMultipleSelection = true; panel.canChooseDirectories = false
        panel.allowedContentTypes = [.init(filenameExtension: "iso") ?? .data]
        panel.message = "Choose ISO files to upload to the hypervisor"
        guard panel.runModal() == .OK else { return }
        let (ok, skipped) = IsoRules.split(panel.urls)
        guard !ok.isEmpty else { error = "Only .iso files can be uploaded"; return }
        ask("Upload ISO", message: "\(ok.count) file\(ok.count == 1 ? "" : "s") selected.", submit: "Continue",
            fields: [PromptField(id: "clash", label: "If the name already exists", initial: "keep it (fail)", options: ["keep it (fail)", "replace it"])]) { [self] v in
            confirmUpload(ok, overwrite: v["clash"] == "replace it", skipped: skipped)
        }
    }

    func promptISODownload() {
        ask("Download ISO to the hypervisor", message: "The hypervisor fetches the file itself, so you can close the app. Only public http(s) URLs.", submit: "Continue",
            fields: [PromptField(id: "url", label: "URL"), PromptField(id: "filename", label: "File name (optional, must end in .iso)"),
                     PromptField(id: "clash", label: "If the name already exists", initial: "keep it (fail)", options: ["keep it (fail)", "replace it"])]) { [self] v in
            let url = v["url"] ?? "", name = v["filename"]?.trimmingCharacters(in: .whitespaces) ?? ""
            do {
                _ = try IsoRules.validate(downloadURL: url)
                if !name.isEmpty { try IsoRules.validate(filename: name) }
            } catch { self.error = error.localizedDescription; return }
            let overwrite = v["clash"] == "replace it"
            confirmAction("Download \(url)?", message: "The hypervisor will download this URL into its ISO directory\(overwrite ? " and replace a file with the same name" : "").",
                          label: "Download", destructive: false) { [self] in
                guard canWrite, let client else { error = "Download ISO: not allowed"; return }
                transfers.download(url: url, filename: name.isEmpty ? nil : name, overwrite: overwrite, client: client)
            }
        }
    }
}

/// Progress rows for uploads and downloads.
struct TransferList: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        let items = model.transfers.items
        if !items.isEmpty {
            VStack(alignment: .leading, spacing: 8) {
                HStack {
                    Text("Transfers").font(.headline)
                    Spacer()
                    Button("Clear finished") { model.transfers.clearFinished() }.disabled(model.transfers.hasRunning && items.allSatisfy { $0.state == .running })
                }
                ForEach(items) { t in
                    HStack(spacing: 10) {
                        Image(systemName: t.kind == .upload ? "arrow.up.circle" : "arrow.down.circle").foregroundStyle(.secondary)
                        VStack(alignment: .leading, spacing: 3) {
                            Text(t.name).lineLimit(1)
                            switch t.state {
                            case .running:
                                if let f = t.fraction { ProgressView(value: f) } else { ProgressView().progressViewStyle(.linear) }
                                Text(detail(t)).font(.caption).foregroundStyle(.secondary).monospacedDigit()
                            case .done: Text("Done").font(.caption).foregroundStyle(.green)
                            case .failed(let m): Text(m).font(.caption).foregroundStyle(.red).textSelection(.enabled)
                            case .cancelled: Text("Cancelled").font(.caption).foregroundStyle(.secondary)
                            }
                        }
                        Spacer()
                        if t.state == .running { Button { model.transfers.cancel(t.id) } label: { Image(systemName: "xmark.circle") }.buttonStyle(.plain).help(t.kind == .upload ? "Cancel upload" : "Stop watching (the server keeps downloading)") }
                    }
                }
            }
            .padding(12).background(.quaternary.opacity(0.4), in: .rect(cornerRadius: 10))
        }
    }

    private func detail(_ t: Transfer) -> String {
        let f = ByteCountFormatter()
        guard let total = t.total else { return t.sent > 0 ? f.string(fromByteCount: t.sent) : "Starting…" }
        return "\(f.string(fromByteCount: t.sent)) of \(f.string(fromByteCount: total))" + (t.fraction.map { String(format: " · %.0f%%", $0 * 100) } ?? "")
    }
}
