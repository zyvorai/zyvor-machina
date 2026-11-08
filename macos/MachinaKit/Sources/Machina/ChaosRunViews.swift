import SwiftUI
import MachinaKit

struct ChaosRunSheet: Identifiable { let id: String; let row: ResourceRow }

/// Run detail: header (status, verdict, who), a timeline of phases (baseline, each step, recovery) with pass or fail
/// against the experiment's abort threshold, findings, and an Abort button while the run is live. Read-only otherwise.
struct ChaosRunDetailView: View {
    @Environment(AppModel.self) private var model
    let runID: String
    let onClose: () -> Void

    @State private var detail: ChaosRunDetail?
    @State private var threshold = 50.0
    @State private var thresholdLoaded = false
    @State private var loadError: String?
    @State private var confirmAbort = false
    @State private var busy = false

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Text("Chaos run").font(.title2.weight(.semibold))
                if let d = detail { statusPill(d.status) }
                Spacer()
                if let d = detail, d.canAbort {
                    Button("Abort run…", role: .destructive) { confirmAbort = true }
                        .disabled(busy || !model.isAdmin).help(model.isAdmin ? "Stops the run and removes its faults" : "Needs the admin role")
                }
                Button("Close") { onClose() }.keyboardShortcut(.cancelAction)
            }
            if let e = loadError { Text(e).foregroundStyle(.red).font(.callout) }
            if let d = detail {
                Grid(alignment: .leading, horizontalSpacing: 12, verticalSpacing: 4) {
                    GridRow { Text("Started").foregroundStyle(.secondary); Text("\(d.startedAt) by \(d.startedBy)") }
                    if let f = d.finishedAt { GridRow { Text("Finished").foregroundStyle(.secondary); Text(f) } }
                    if !d.verdict.isEmpty { GridRow { Text("Verdict").foregroundStyle(.secondary); Text(d.verdict).bold() } }
                    if !d.abortReason.isEmpty { GridRow { Text("Abort reason").foregroundStyle(.secondary); Text(d.abortReason) } }
                    if !d.targets.isEmpty { GridRow { Text("Targets").foregroundStyle(.secondary); Text(d.targets.joined(separator: ", ")) } }
                    if !d.probes.isEmpty { GridRow { Text("Probes").foregroundStyle(.secondary); Text(d.probes.joined(separator: ", ")) } }
                }.font(.callout)
                if d.phases.isEmpty {
                    ContentUnavailableView(d.canAbort ? "Waiting for the first phase" : "No phase report", systemImage: "timeline.selection")
                        .frame(maxHeight: .infinity)
                } else {
                    Text("\(d.passedPhases) of \(d.phases.count) phases within the \(Int(threshold))% probe threshold").font(.headline)
                    ScrollView { timeline(d) }
                }
                if !d.findings.isEmpty {
                    GroupBox("Findings") {
                        VStack(alignment: .leading, spacing: 4) {
                            ForEach(Array(d.findings.enumerated()), id: \.offset) { _, f in Text("• \(f)").textSelection(.enabled) }
                        }.frame(maxWidth: .infinity, alignment: .leading)
                    }
                }
            } else if loadError == nil {
                ProgressView().frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        .padding(20).frame(width: 720, height: 640)
        .task { await refreshLoop() }
        .alert("Abort this run?", isPresented: $confirmAbort) {
            Button("Abort run", role: .destructive) { abort() }
            Button("Cancel", role: .cancel) {}
        } message: { Text("Stops the experiment now and removes its faults from the targets.") }
    }

    private func timeline(_ d: ChaosRunDetail) -> some View {
        let total = max(d.phases.map(\.endedS).max() ?? 1, 1)
        return VStack(alignment: .leading, spacing: 8) {
            ForEach(d.phases) { p in
                let ok = p.passed(minSuccessPct: threshold)
                let running = d.canAbort && d.livePhase == p.id
                VStack(alignment: .leading, spacing: 3) {
                    HStack {
                        Image(systemName: running ? "hourglass" : (ok ? "checkmark.circle.fill" : "xmark.octagon.fill"))
                            .foregroundStyle(running ? Color.orange : (ok ? Color.green : Color.red))
                            .accessibilityLabel(running ? "Running" : (ok ? "Passed" : "Failed"))
                        Text(p.name).fontWeight(.medium)
                        Spacer()
                        Text(summary(p)).font(.caption).foregroundStyle(.secondary).monospacedDigit()
                    }
                    GeometryReader { g in
                        ZStack(alignment: .leading) {
                            RoundedRectangle(cornerRadius: 3).fill(.quaternary)
                            RoundedRectangle(cornerRadius: 3).fill(ok ? Color.green.opacity(0.6) : Color.red.opacity(0.6))
                                .frame(width: max(4, g.size.width * CGFloat(p.seconds / total)))
                                .offset(x: g.size.width * CGFloat(p.startedS / total))
                        }
                    }.frame(height: 8)
                    if let e = p.error { Text(e).font(.caption).foregroundStyle(.red) }
                    if let r = p.recoveredS { Text("Recovered after \(String(format: "%.1f", r))s").font(.caption).foregroundStyle(.secondary) }
                    ForEach(Array(p.notes.enumerated()), id: \.offset) { _, n in Text(n).font(.caption).foregroundStyle(.secondary) }
                }
            }
        }.padding(.trailing, 8)
    }

    private func summary(_ p: ChaosPhase) -> String {
        var parts = ["\(String(format: "%.0f", p.startedS))s to \(String(format: "%.0f", p.endedS))s"]
        if p.samples > 0 {
            parts.append("\(p.ok)/\(p.samples) probes" + (p.successPct.map { " (\(String(format: "%.0f", $0))%)" } ?? ""))
            if let a = p.p50ms, let b = p.p95ms { parts.append("p50 \(a) ms, p95 \(b) ms") }
        }
        return parts.joined(separator: " · ")
    }

    private func statusPill(_ s: String) -> some View {
        Text(s).font(.caption.weight(.semibold)).padding(.horizontal, 8).padding(.vertical, 2)
            .background(Capsule().fill(s == "running" ? Color.orange.opacity(0.25) : (s == "completed" || s == "passed" ? Color.green.opacity(0.25) : Color.red.opacity(0.2))))
    }

    private func refreshLoop() async {
        while !Task.isCancelled {
            await load()
            guard detail?.canAbort == true else { return }
            try? await Task.sleep(nanoseconds: 3_000_000_000)
        }
    }

    private func load() async {
        do {
            let json = try await model.call(ChaosAPI.getRun(id: runID))
            let d = ChaosRunDetail(json: json)
            detail = d; loadError = nil
            if !thresholdLoaded, !d.experimentID.isEmpty { thresholdLoaded = true; if let ex = try? await model.call(.controllerGet("/api/v1/chaos/experiments/\(d.experimentID)")) {
                threshold = ChaosDraft(row: ex).minSuccessPct }
            }
        } catch { loadError = error.localizedDescription }
    }

    private func abort() {
        busy = true
        Task {
            defer { busy = false }
            do { _ = try await model.call(ChaosAPI.abort(runID: runID)); model.notice = "Abort requested"; await load() }
            catch { loadError = error.localizedDescription }
        }
    }
}
