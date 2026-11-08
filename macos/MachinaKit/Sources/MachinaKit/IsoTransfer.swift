import Foundation

// ISO upload and download, confirmed against the daemon (`daemon/src/routes/extras.rs`, `web/src/api/extras.ts`):
//   POST /api/v1/browse/isos/upload?filename=<bare name>[&overwrite=true]   body: raw bytes, one request, no job
//   POST /api/v1/browse/isos/download {url, filename?, overwrite?}          -> {status, job_id, name, path}
//   GET  /api/v1/jobs/{id}                                                  -> {status: running|completed|failed, bytes_done?, bytes_total?, error?}
//   GET  /api/v1/browse/isos                                                -> {files: [{path, name, size_bytes, format}], scan_directories}
// The upload is a single streaming request (the web UI reports XHR upload progress); only the server-side
// download is a job. The daemon has no ISO delete route.

public struct IsoFile: Codable, Hashable, Identifiable, Sendable {
    public var path: String
    public var name: String
    public var sizeBytes: Int64
    public var format: String
    public var id: String { path }
    enum CodingKeys: String, CodingKey { case path, name, format, sizeBytes = "size_bytes" }
}

public struct IsoUploadResult: Codable, Hashable, Sendable {
    public var status: String
    public var name: String
    public var path: String
    public var sizeBytes: Int64
    enum CodingKeys: String, CodingKey { case status, name, path, sizeBytes = "size_bytes" }
}

public struct IsoDownloadStarted: Codable, Hashable, Sendable {
    public var status: String
    public var jobId: String
    public var name: String
    public var path: String
    enum CodingKeys: String, CodingKey { case status, name, path, jobId = "job_id" }
}

/// `GET /api/v1/jobs/{id}`: the fields a progress bar needs.
public struct JobProgress: Codable, Hashable, Identifiable, Sendable {
    public var id: String
    public var status: String
    public var title: String?
    public var error: String?
    public var bytesDone: Int64?
    public var bytesTotal: Int64?
    enum CodingKeys: String, CodingKey { case id, status, title, error, bytesDone = "bytes_done", bytesTotal = "bytes_total" }

    public var isRunning: Bool { status == "running" }
    public var isFailed: Bool { status == "failed" }
    /// nil when the server did not advertise a size: show an indeterminate bar, never a wrong percentage.
    public var fraction: Double? {
        guard let done = bytesDone, let total = bytesTotal, total > 0 else { return nil }
        return min(1, max(0, Double(done) / Double(total)))
    }
}

public enum IsoRules {
    public struct Invalid: Error, LocalizedError, Sendable { public var message: String; public var errorDescription: String? { message } }

    /// Mirrors `machina_core::iso_upload::sanitize_iso_filename`: a bare `.iso` name, no separators, no leading `.` or `-`.
    public static func validate(filename raw: String) throws {
        let name = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        if name.isEmpty { throw Invalid(message: "A file name is required.") }
        if name.utf8.count > 255 { throw Invalid(message: "The file name must be at most 255 bytes.") }
        if name.contains("/") || name.contains("\\") { throw Invalid(message: "The file name must not contain a path separator.") }
        if name.hasPrefix(".") || name.hasPrefix("-") { throw Invalid(message: "The file name must not start with '.' or '-'.") }
        guard name.lowercased().hasSuffix(".iso"), name.count > 4 else { throw Invalid(message: "Only .iso files can be uploaded.") }
    }

    /// Splits dropped files into uploadable ISOs and the names that were skipped (folders, other types).
    public static func split(_ urls: [URL]) -> (accepted: [URL], skipped: [String]) {
        var ok: [URL] = [], skipped: [String] = []
        for u in urls {
            var isDir: ObjCBool = false
            let exists = FileManager.default.fileExists(atPath: u.path, isDirectory: &isDir)
            if u.isFileURL, exists, !isDir.boolValue, (try? validate(filename: u.lastPathComponent)) != nil { ok.append(u) }
            else { skipped.append(u.lastPathComponent) }
        }
        return (ok, skipped)
    }

    /// The daemon accepts plain http(s) only (and refuses internal hosts itself).
    public static func validate(downloadURL raw: String) throws -> URL {
        let s = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        guard let u = URL(string: s), let scheme = u.scheme?.lowercased(), scheme == "http" || scheme == "https", u.host?.isEmpty == false else {
            throw Invalid(message: "The URL must start with http:// or https://.")
        }
        return u
    }

    /// A readable message from a daemon error body (`{"error": "..."}`), else the raw text.
    static func message(from data: Data) -> String {
        if let v = try? JSONDecoder().decode(JSONValue.self, from: data), let e = v["error"]?.text ?? v["message"]?.text, e != "–" { return e }
        return String(decoding: data.prefix(300), as: UTF8.self)
    }
}

/// Holds the in-flight upload task so the progress poller and the cancellation handler can reach it.
private final class UploadBox: @unchecked Sendable {
    private let lock = NSLock()
    private var task: URLSessionTask?
    private var cancelled = false
    func set(_ t: URLSessionTask) { lock.withLock { task = t; if cancelled { t.cancel() } } }
    func cancel() { lock.withLock { cancelled = true; task?.cancel() } }
    var sent: Int64 { lock.withLock { task?.countOfBytesSent ?? 0 } }
}

public extension MachinaClient {
    func listIsos() async throws -> [IsoFile] {
        struct R: Decodable { var files: [IsoFile] }
        return try JSONDecoder().decode(R.self, from: try await send("GET", "/api/v1/browse/isos")).files
    }

    /// Streams `file` to the daemon's ISO upload directory. `progress(sent, total)` is called about 4 times a second.
    /// The whole file is one request body read from disk by URLSession (never held in memory); cancel by cancelling the Task.
    @discardableResult
    func uploadIso(file: URL, name: String? = nil, overwrite: Bool = false,
                   progress: @escaping @Sendable (Int64, Int64) -> Void = { _, _ in }) async throws -> IsoUploadResult {
        let filename = name ?? file.lastPathComponent
        try IsoRules.validate(filename: filename)
        let total = (try FileManager.default.attributesOfItem(atPath: file.path)[.size] as? NSNumber)?.int64Value ?? 0
        guard total > 0 else { throw IsoRules.Invalid(message: "The file is empty.") }
        var q = [URLQueryItem(name: "filename", value: filename)]
        if overwrite { q.append(URLQueryItem(name: "overwrite", value: "true")) }
        var retried = false
        while true {
            var req = URLRequest(url: try url("/api/v1/browse/isos/upload", q))
            req.httpMethod = "POST"
            req.setValue("application/octet-stream", forHTTPHeaderField: "Content-Type")
            req.timeoutInterval = 900   // the daemon answers only after fsync + rename of the whole file
            let box = UploadBox()
            let poller = Task { while !Task.isCancelled { progress(box.sent, total); try? await Task.sleep(nanoseconds: 250_000_000) } }
            defer { poller.cancel() }
            let sess = urlSession
            let r = req
            let (data, resp): (Data, URLResponse) = try await withTaskCancellationHandler {
                try await withCheckedThrowingContinuation { cont in
                    let t = sess.uploadTask(with: r, fromFile: file) { data, resp, err in
                        if let err { cont.resume(throwing: err) }
                        else if let data, let resp { cont.resume(returning: (data, resp)) }
                        else { cont.resume(throwing: URLError(.badServerResponse)) }
                    }
                    box.set(t); t.resume()
                }
            } onCancel: { box.cancel() }
            let code = (resp as? HTTPURLResponse)?.statusCode ?? 0
            if code == 401, !retried, try await reloginIfPossible() { retried = true; continue }
            guard (200..<300).contains(code) else {
                throw code == 401 ? MachinaError.unauthenticated : MachinaError.http(code, IsoRules.message(from: data))
            }
            progress(total, total)
            return try JSONDecoder().decode(IsoUploadResult.self, from: data)
        }
    }

    /// Starts a server-side download and returns the job to poll.
    func startIsoDownload(url source: String, filename: String? = nil, overwrite: Bool = false) async throws -> IsoDownloadStarted {
        let u = try IsoRules.validate(downloadURL: source)
        var body: [String: JSONValue] = ["url": .string(u.absoluteString), "overwrite": .bool(overwrite)]
        if let f = filename?.trimmingCharacters(in: .whitespacesAndNewlines), !f.isEmpty { try IsoRules.validate(filename: f); body["filename"] = .string(f) }
        let data = try await send("POST", "/api/v1/browse/isos/download", body: try JSONEncoder().encode(JSONValue.object(body)))
        return try JSONDecoder().decode(IsoDownloadStarted.self, from: data)
    }

    func job(_ id: String) async throws -> JobProgress {
        try JSONDecoder().decode(JobProgress.self, from: try await send("GET", "/api/v1/jobs/\(Self.seg(id))"))
    }

    /// Polls a job until it is no longer running, reporting each sample. Throws when the job failed or the Task was cancelled.
    @discardableResult
    func waitForJob(_ id: String, interval: Duration = .seconds(1), update: @Sendable (JobProgress) -> Void = { _ in }) async throws -> JobProgress {
        while true {
            try Task.checkCancellation()
            let j = try await job(id)
            update(j)
            if j.isFailed { throw IsoRules.Invalid(message: j.error ?? "The download failed.") }
            if !j.isRunning { return j }
            try await Task.sleep(for: interval)
        }
    }
}
