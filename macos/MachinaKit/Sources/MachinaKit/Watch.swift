import Foundation

private struct WatchMessage: Decodable {
    var event: String
    var changes: [VmChange]?
}

public extension MachinaClient {
    /// Live VM changes. Reconnects with backoff and a fresh single-use token each time.
    nonisolated func watch() -> AsyncStream<WatchEvent> {
        AsyncStream { continuation in
            let task = Task {
                var delay: UInt64 = 1
                while !Task.isCancelled {
                    do {
                        let token = try await self.wsToken()
                        let ws = await self.urlSession.webSocketTask(with: try await self.watchURL(token: token))
                        ws.resume()
                        defer { ws.cancel(with: .goingAway, reason: nil) }
                        while !Task.isCancelled {
                            let msg = try await ws.receive()
                            var data: Data?
                            switch msg { case .string(let s): data = Data(s.utf8); case .data(let d): data = d; @unknown default: break }
                            guard let data, let m = try? JSONDecoder().decode(WatchMessage.self, from: data) else { continue }
                            delay = 1
                            continuation.yield(m.event == "changes" ? .changes(m.changes ?? []) : .heartbeat)
                        }
                    } catch {
                        continuation.yield(.disconnected)
                    }
                    try? await Task.sleep(nanoseconds: delay * 1_000_000_000)
                    delay = min(delay * 2, 30)
                }
                continuation.finish()
            }
            continuation.onTermination = { _ in task.cancel() }
        }
    }
}
