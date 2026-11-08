import SwiftUI
import Charts
import AppKit
import MachinaKit

/// Insight > AI governance: cost attribution and budget, compliance frameworks, one-at-a-time right-sizing,
/// the digest schedule, intent environments and exports. Every write is admin-only and typed-confirmed.
struct AIGovernanceView: View {
    enum Tab: String, CaseIterable, Identifiable { case cost = "Cost", compliance = "Compliance", rightsizing = "Right-sizing", digest = "Digest", intent = "Intent", exports = "Exports"
        var id: String { rawValue } }
    @State private var tab = Tab.cost

    var body: some View {
        VStack(spacing: 0) {
            Picker("", selection: $tab) { ForEach(Tab.allCases) { Text($0.rawValue).tag($0) } }
                .pickerStyle(.segmented).labelsHidden().padding(12)
            Divider()
            Group {
                switch tab {
                case .cost: AICostPanel()
                case .compliance: AICompliancePanel()
                case .rightsizing: AIRightsizingPanel()
                case .digest: AIDigestPanel()
                case .intent: AIIntentPanel()
                case .exports: AIExportsPanel()
                }
            }.frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        }.navigationTitle("AI governance")
    }
}

/// A sheet that runs one action only after the exact phrase is typed.
struct TypedConfirmSheet: View {
    let title: String
    let message: String
    let phrase: String
    let actionLabel: String
    let onCancel: () -> Void
    let onConfirm: () -> Void
    @State private var typed = ""
    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(title).font(.title3.weight(.semibold))
            Text(message).font(.callout).fixedSize(horizontal: false, vertical: true)
            Text("Type \(phrase) to confirm").font(.caption).foregroundStyle(.secondary)
            TextField(phrase, text: $typed).textFieldStyle(.roundedBorder)
            HStack {
                Spacer()
                Button("Cancel", action: onCancel).keyboardShortcut(.cancelAction)
                Button(actionLabel, role: .destructive, action: onConfirm).disabled(typed != phrase)
            }
        }.padding(20).frame(width: 460)
    }
}

private struct Pending: Identifiable { let id = UUID(); let title: String; let message: String; let phrase: String; let label: String; let run: @MainActor () async -> Void }

// MARK: Cost

private struct AICostPanel: View {
    @Environment(AppModel.self) private var model
    @State private var attr: CostAttribution?
    @State private var budget: CostBudget?
    @State private var error: String?

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                if let e = error { Text(e).foregroundStyle(.red).font(.callout) }
                if let b = budget {
                    GroupBox("Monthly budget (read-only)") {
                        VStack(alignment: .leading, spacing: 6) {
                            ProgressView(value: min(1, max(0, b.utilizationPct / 100)))
                            Text(String(format: "%.0f USD spent of %.0f USD (%.0f%%), predicted %.0f USD. Status: %@", b.spendUSD, b.budgetUSD, b.utilizationPct, b.predictedUSD, b.status)).font(.callout)
                            ForEach(b.alerts, id: \.self) { Text($0).font(.caption).foregroundStyle(.orange) }
                            Text(CostBudget.setNote).font(.caption).foregroundStyle(.secondary)
                        }.frame(maxWidth: .infinity, alignment: .leading)
                    }
                }
                if let a = attr {
                    Text("Cost by team").font(.headline)
                    Text(a.summary).font(.callout).foregroundStyle(.secondary)
                    Chart(a.chartRows, id: \.name) { r in
                        BarMark(x: .value("USD / month", r.usd), y: .value("Team", r.name)).foregroundStyle(r.name == "Unattributed" ? Color.gray : Color.accentColor)
                            .annotation(position: .trailing) { Text(String(format: "%.0f", r.usd)).font(.caption2) }
                    }.frame(height: CGFloat(max(120, a.chartRows.count * 34)))
                    Table(a.teams) {
                        TableColumn("Team") { Text($0.team.isEmpty ? "(untagged)" : $0.team) }
                        TableColumn("VMs") { Text("\($0.vmCount)") }
                        TableColumn("vCPUs") { Text("\($0.vcpus)") }
                        TableColumn("Memory GiB") { Text(String(format: "%.1f", $0.memoryGiB)) }
                        TableColumn("USD / month") { Text(String(format: "%.0f", $0.monthlyUSD)) }
                        TableColumn("Share") { Text(String(format: "%.0f%%", $0.sharePct)) }
                    }.frame(height: CGFloat(min(320, 40 + a.teams.count * 28)))
                } else if error == nil { ProgressView() }
            }.padding(16)
        }
        .toolbar { Button("Refresh") { Task { await load() } } }
        .task { await load() }
    }

    private func load() async {
        do {
            attr = CostAttribution(json: try await model.call(AIGov.attribution))
            budget = CostBudget(json: try await model.call(AIGov.budget)); error = nil
        } catch { self.error = error.localizedDescription }
    }
}

// MARK: Compliance

private struct AICompliancePanel: View {
    @Environment(AppModel.self) private var model
    @State private var report: ComplianceFrameworks?
    @State private var proposals: [RemediationProposal] = []
    @State private var rejected = 0
    @State private var selected: String?
    @State private var previewing = false
    @State private var error: String?

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 14) {
                if let e = error { Text(e).foregroundStyle(.red).font(.callout) }
                if let r = report {
                    Text(r.summary).font(.callout).foregroundStyle(.secondary)
                    Chart(r.frameworks) { f in
                        BarMark(x: .value("Score", f.score), y: .value("Framework", f.name))
                            .foregroundStyle(f.score >= 80 ? Color.green : (f.score >= 60 ? Color.orange : Color.red))
                            .annotation(position: .trailing) { Text("\(Int(f.score)) \(f.grade)").font(.caption2) }
                    }.chartXScale(domain: 0...100).frame(height: CGFloat(max(100, r.frameworks.count * 40)))
                    ForEach(r.frameworks) { f in
                        DisclosureGroup("\(f.name): \(Int(f.score)) (\(f.grade)), \(f.failed) of \(f.controls) controls failing") {
                            ForEach(r.failedControls(in: f.name)) { c in
                                VStack(alignment: .leading) { Text(c.title).bold(); Text(c.detail).font(.caption).foregroundStyle(.secondary) }.padding(.vertical, 2)
                            }
                        }
                    }
                } else if error == nil { ProgressView() }
                Divider()
                HStack {
                    Text("Remediation").font(.headline)
                    Spacer()
                    Button("Preview remediation") { Task { await preview() } }.disabled(!model.isAdmin || previewing)
                        .help(model.isAdmin ? "Lists what the controller proposes. Changes nothing." : AIGov.adminOnlyNote)
                }
                if !model.isAdmin { Text(AIGov.adminOnlyNote).font(.caption).foregroundStyle(.secondary) }
                if previewing {
                    Text(ComplianceRemediation.applyNote).font(.callout).foregroundStyle(.orange)
                    if rejected > 0 { Text("\(rejected) proposal(s) with an unknown action were dropped.").font(.caption).foregroundStyle(.secondary) }
                    if proposals.isEmpty { Text("No remediations proposed.").foregroundStyle(.secondary) }
                    ForEach(proposals) { p in
                        VStack(alignment: .leading, spacing: 2) {
                            Text(p.label).bold()
                            Text("\(p.framework) · risk \(p.risk) · \(ComplianceRemediation.effect(p.action))").font(.caption)
                            if !p.review.isEmpty { Text(p.review).font(.caption).foregroundStyle(.secondary) }
                        }.padding(8).frame(maxWidth: .infinity, alignment: .leading).background(.quaternary.opacity(0.4), in: RoundedRectangle(cornerRadius: 6))
                    }
                }
            }.padding(16)
        }
        .toolbar { Button("Refresh") { Task { await load() } } }
        .task { await load() }
    }

    private func load() async {
        do { report = ComplianceFrameworks(json: try await model.call(AIGov.frameworks)); error = nil } catch { self.error = error.localizedDescription }
    }
    private func preview() async {
        guard model.isAdmin else { return }
        do { let r = ComplianceRemediation.parse(try await model.call(AIGov.remediate)); proposals = r.proposals; rejected = r.rejected; previewing = true }
        catch { self.error = error.localizedDescription }
    }
}

// MARK: Right-sizing

private struct AIRightsizingPanel: View {
    @Environment(AppModel.self) private var model
    @State private var recs: [RightsizingRec] = []
    @State private var loaded = false
    @State private var error: String?
    @State private var pending: Pending?
    @State private var busy = false

    var body: some View {
        List {
            if let e = error { Text(e).foregroundStyle(.red) }
            Text("Apply one recommendation at a time. Each shows the before and after and needs its VM name typed. There is no bulk apply.").font(.callout).foregroundStyle(.secondary)
            if loaded && recs.isEmpty { Text("Nothing to right-size.").foregroundStyle(.secondary) }
            ForEach(recs) { r in
                let problem = RightsizingApply.problem(r)
                VStack(alignment: .leading, spacing: 4) {
                    HStack { Text(r.vmName.isEmpty ? "(unnamed)" : r.vmName).bold(); Spacer(); Text(String(format: "saves %.0f USD / month", r.savingsUSD)).font(.caption) }
                    ForEach(RightsizingApply.beforeAfter(r, vcpus: model.vms.first { $0.name == r.vmName }?.vcpus), id: \.label) { row in
                        Text("\(row.label): \(row.before) -> \(row.after)").font(.callout.monospacedDigit())
                    }
                    Text(r.detail).font(.caption).foregroundStyle(.secondary)
                    HStack {
                        if let p = problem { Text(p).font(.caption).foregroundStyle(.secondary) }
                        Spacer()
                        Button("Apply…") { ask(r) }.disabled(problem != nil || !model.isAdmin || busy)
                            .help(model.isAdmin ? "" : AIGov.adminOnlyNote)
                    }
                }.padding(.vertical, 4)
            }
        }
        .toolbar { Button("Refresh") { Task { await load() } } }
        .task { await load() }
        .sheet(item: $pending) { p in
            TypedConfirmSheet(title: p.title, message: p.message, phrase: p.phrase, actionLabel: p.label, onCancel: { pending = nil }) {
                pending = nil; Task { await p.run() }
            }
        }
    }

    private func ask(_ r: RightsizingRec) {
        guard model.isAdmin, let e = try? RightsizingApply.endpoint(r) else { return }
        pending = Pending(title: "Resize \(r.vmName)", message: RightsizingApply.confirmText(r), phrase: RightsizingApply.phrase(r), label: "Resize") {
            busy = true; defer { busy = false }
            do { _ = try await model.call(e); model.notice = "Resize of \(r.vmName) queued"; await load() } catch { self.error = error.localizedDescription }
        }
    }
    private func load() async {
        do { recs = RightsizingRec.parse(try await model.call(AIGov.rightsizing)); loaded = true; error = nil } catch { self.error = error.localizedDescription }
    }
}

// MARK: Digest

private struct AIDigestPanel: View {
    @Environment(AppModel.self) private var model
    @State private var schedule = DigestSchedule()
    @State private var loaded = false
    @State private var error: String?
    @State private var pending: Pending?

    var body: some View {
        Form {
            if let e = error { Text(e).foregroundStyle(.red) }
            Toggle("Send the digest on a schedule", isOn: $schedule.enabled)
            Picker("Frequency", selection: $schedule.frequency) { ForEach(DigestSchedule.frequencies, id: \.self) { Text($0.capitalized).tag($0) } }
            Picker("Hour (UTC)", selection: $schedule.hourUTC) { ForEach(0..<24, id: \.self) { Text(String(format: "%02d:00", $0)).tag($0) } }
            if schedule.frequency == "weekly" {
                Picker("Day", selection: $schedule.weekday) { ForEach(1...7, id: \.self) { Text(DigestSchedule.weekdayNames[$0 - 1]).tag($0) } }
            }
            if let l = schedule.lastSentAt { Text("Last sent \(l)").font(.caption).foregroundStyle(.secondary) }
            if !model.isAdmin { Text(AIGov.adminOnlyNote).font(.caption).foregroundStyle(.secondary) }
            HStack {
                Button("Save schedule…") { save() }.disabled(!model.isAdmin || !loaded)
                Button("Send now…") { sendNow() }.disabled(!model.isAdmin)
            }
        }.formStyle(.grouped)
        .task { await load() }
        .sheet(item: $pending) { p in
            TypedConfirmSheet(title: p.title, message: p.message, phrase: p.phrase, actionLabel: p.label, onCancel: { pending = nil }) {
                pending = nil; Task { await p.run() }
            }
        }
    }

    private func load() async {
        do { schedule = DigestSchedule(json: try await model.call(AIGov.digestSchedule)); loaded = true; error = nil } catch { self.error = error.localizedDescription }
    }
    private func save() {
        guard model.isAdmin, let e = try? schedule.putEndpoint() else { error = schedule.problem; return }
        pending = Pending(title: "Save digest schedule", message: schedule.confirmText, phrase: "SAVE SCHEDULE", label: "Save") {
            do { schedule = DigestSchedule(json: try await model.call(e)); model.notice = "Digest schedule saved"; error = nil } catch { self.error = error.localizedDescription }
        }
    }
    private func sendNow() {
        guard model.isAdmin else { return }
        pending = Pending(title: "Send the digest now", message: DigestSchedule.sendText, phrase: DigestSchedule.sendPhrase, label: "Send") {
            do { _ = try await model.call(AIGov.digestSend); model.notice = "Digest sent"; await load() } catch { self.error = error.localizedDescription }
        }
    }
}

// MARK: Intent

private struct AIIntentPanel: View {
    @Environment(AppModel.self) private var model
    @State private var query = ""
    @State private var plan: EnvironmentPlan?
    @State private var planQuery = ""
    @State private var refused: [String] = []
    @State private var placements: [(name: String, host: String)] = []
    @State private var dryRan = false
    @State private var error: String?
    @State private var pending: Pending?
    @State private var busy = false

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 12) {
                Text("Describe an environment. The generated plan is shown for review; nothing runs until you check placement and type the confirmation. One plan at a time.").font(.callout).foregroundStyle(.secondary)
                TextField("e.g. a dev environment for 3 developers", text: $query, axis: .vertical).textFieldStyle(.roundedBorder).lineLimit(2...4)
                HStack {
                    Button("Generate plan") { Task { await generate() } }.disabled(busy || EnvironmentIntent.queryProblem(query) != nil)
                    if let p = EnvironmentIntent.queryProblem(query), !query.isEmpty { Text(p).font(.caption).foregroundStyle(.orange) }
                }
                if let e = error { Text(e).foregroundStyle(.red).font(.callout) }
                if !refused.isEmpty {
                    GroupBox("Plan refused") { VStack(alignment: .leading) { ForEach(refused, id: \.self) { Text("• \($0)") } }.frame(maxWidth: .infinity, alignment: .leading) }
                }
                if let p = plan {
                    GroupBox(p.label.isEmpty ? "Plan" : p.label) {
                        VStack(alignment: .leading, spacing: 6) {
                            Text(p.review).font(.callout)
                            Text("\(p.vmCount) VM(s) of \(p.vcpusPerVM) vCPU and \(p.memoryGiBPerVM) GiB, \(p.storageGiB) GiB storage, about \(Int(p.monthlyUSD)) USD per month").font(.callout.monospacedDigit())
                            Text("Steps (read-only):").font(.caption).foregroundStyle(.secondary)
                            ForEach(Array(p.steps.enumerated()), id: \.offset) { i, s in Text("\(i + 1). \(s)").font(.callout).textSelection(.enabled) }
                        }.frame(maxWidth: .infinity, alignment: .leading)
                    }
                    HStack {
                        Button("Check placement") { Task { await dryRun(p) } }.disabled(busy)
                        Button("Create VMs…") { ask(p) }.disabled(busy || !dryRan || placements.isEmpty || !model.isAdmin)
                            .help(model.isAdmin ? (dryRan ? "" : "Check placement first") : AIGov.adminOnlyNote)
                    }
                    if dryRan {
                        GroupBox("Placement (nothing created yet)") {
                            VStack(alignment: .leading) { ForEach(placements, id: \.name) { Text("\($0.name) on \($0.host)").font(.callout.monospaced()) } }
                                .frame(maxWidth: .infinity, alignment: .leading)
                        }
                    }
                }
            }.padding(16)
        }
        .sheet(item: $pending) { p in
            TypedConfirmSheet(title: p.title, message: p.message, phrase: p.phrase, actionLabel: p.label, onCancel: { pending = nil }) {
                pending = nil; Task { await p.run() }
            }
        }
    }

    private func generate() async {
        busy = true; defer { busy = false }
        plan = nil; refused = []; placements = []; dryRan = false; error = nil
        do {
            switch EnvironmentIntent.validated(try await model.call(EnvironmentIntent.planEndpoint(query: query))) {
            case .success(let p): plan = p; planQuery = query
            case .failure(let r): refused = r.reasons
            }
        } catch { self.error = error.localizedDescription }
    }
    private func dryRun(_ p: EnvironmentPlan) async {
        busy = true; defer { busy = false }
        do {
            let r = try await model.call(EnvironmentIntent.executeEndpoint(query: planQuery, dryRun: true, maxVMs: p.vmCount))
            placements = EnvironmentIntent.placements(r); dryRan = true; error = nil
        } catch { self.error = error.localizedDescription }
    }
    private func ask(_ p: EnvironmentPlan) {
        let n = placements.count
        guard model.isAdmin, n > 0, let e = try? EnvironmentIntent.executeEndpoint(query: planQuery, dryRun: false, maxVMs: n) else { return }
        pending = Pending(title: "Create environment", message: EnvironmentIntent.confirmText(p, vms: n), phrase: EnvironmentIntent.phrase(p, vms: n), label: "Create") {
            busy = true; defer { busy = false }
            do { _ = try await model.call(e); model.notice = "Environment creation queued"; plan = nil; placements = []; dryRan = false } catch { self.error = error.localizedDescription }
        }
    }
}

// MARK: Exports

private struct AIExportsPanel: View {
    @Environment(AppModel.self) private var model
    @State private var busyID: String?
    @State private var error: String?

    var body: some View {
        List {
            if let e = error { Text(e).foregroundStyle(.red) }
            ForEach(AIGov.exports) { e in
                HStack {
                    Text(e.title); Spacer()
                    Button(busyID == e.id ? "Saving…" : "Save as…") { Task { await save(e) } }.disabled(busyID != nil)
                }
            }
        }
    }

    private func save(_ e: AIGov.Export) async {
        busyID = e.id; defer { busyID = nil }
        do {
            let data = try await model.api().exportData(e)
            let panel = NSSavePanel(); panel.nameFieldStringValue = e.fileName
            guard panel.runModal() == .OK, let url = panel.url else { return }
            try data.write(to: url, options: .atomic)
            model.notice = "Saved \(url.lastPathComponent)"; error = nil
        } catch { self.error = error.localizedDescription }
    }
}
