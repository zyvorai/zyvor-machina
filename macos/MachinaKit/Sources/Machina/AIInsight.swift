import SwiftUI
import Charts
import MachinaKit

/// Pages for the AI insight endpoints. All read-only except the two admin-only single-VM executes
/// (autopilot proposal, one rebalance move), each previewed and confirmed by typing the action and VM name.
struct AIInsightRouter: View {
    let page: InsightPage
    var body: some View {
        switch page {
        case .aiBlackBox: BlackBoxPage()
        case .aiPerformance: PerformancePage()
        case .aiMigration: MigrationOraclePage()
        case .aiFleet: FleetInsightPage()
        case .aiNoisy: NoisyGraphPage()
        default: EmptyView()
        }
    }
}

private struct AIFrame<Content: View>: View {
    @Environment(AppModel.self) private var model
    let title: String, subtitle: String, caveat: String
    @ViewBuilder var content: Content
    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                VStack(alignment: .leading, spacing: 2) {
                    Text(title).font(.largeTitle.weight(.semibold))
                    Text(subtitle).foregroundStyle(.secondary)
                }
                Label(caveat, systemImage: "exclamationmark.triangle").font(.callout).foregroundStyle(.orange)
                    .padding(10).frame(maxWidth: .infinity, alignment: .leading)
                    .background(.orange.opacity(0.1), in: RoundedRectangle(cornerRadius: 8))
                PlatformErrorBanner(text: model.platformError)
                content
            }.padding(24).frame(maxWidth: .infinity, alignment: .leading)
        }
    }
}

private func bullets(_ items: [String]) -> some View {
    VStack(alignment: .leading, spacing: 3) { ForEach(Array(items.enumerated()), id: \.offset) { _, t in Text("• \(t)").textSelection(.enabled) } }
        .frame(maxWidth: .infinity, alignment: .leading)
}

private struct ErrorLine: View {
    let text: String?
    var body: some View { if let text { Text(text).foregroundStyle(.red).font(.callout).textSelection(.enabled) } }
}

private func toneForScore(_ s: Double) -> Color { s >= 70 ? .red : s >= 40 ? .orange : .green }

// MARK: Black Box

private struct BlackBoxPage: View {
    @Environment(AppModel.self) private var model
    @State private var vm = ""
    @State private var report: BlackBoxReport?
    @State private var error: String?
    @State private var busy = false
    @State private var trigger = false
    @State private var note: String?

    var body: some View {
        AIFrame(title: "Black Box root cause", subtitle: "Ranked hypotheses from a VM's frozen eBPF capture.", caveat: AIInsightCopy.blackBox) {
            HStack {
                Picker("VM", selection: $vm) {
                    Text("Choose a VM").tag("")
                    ForEach(model.vms.map(\.name), id: \.self) { Text($0).tag($0) }
                }.frame(maxWidth: 320)
                Button("Analyse") { Task { await load() } }.disabled(vm.isEmpty || busy)
                Spacer()
                Button("Freeze capture now…") { trigger = true }.disabled(vm.isEmpty || !model.isAdmin)
                    .help(model.isAdmin ? "Freezes the rolling buffer for this VM on the host you are connected to" : "Needs the admin role")
            }
            ErrorLine(text: error)
            if let note { Text(note).font(.callout).foregroundStyle(.secondary) }
            if busy { ProgressView() }
            if let r = report { reportView(r) }
        }
        .confirmationDialog("Freeze the Black Box for \(vm)?", isPresented: $trigger, titleVisibility: .visible) {
            Button("Freeze capture") { Task { await freeze() } }
            Button("Cancel", role: .cancel) {}
        } message: { Text("bpfd keeps the last 120 s of events for \(vm) and records 15 s more. It changes nothing on the VM. Reload the analysis after about 20 s.") }
    }

    @ViewBuilder private func reportView(_ r: BlackBoxReport) -> some View {
        if !r.hasCapture {
            ContentUnavailableView("No frozen capture for \(r.vm)", systemImage: "shippingbox",
                                   description: Text("The buffer freezes on a trigger or on a high or critical anomaly. Freeze one now, then analyse again."))
        } else {
            GroupBox("Most likely cause") {
                VStack(alignment: .leading, spacing: 6) {
                    Text(r.rootCause).font(.title3.weight(.semibold))
                    Text("Lead over the alternatives: \(Int(r.confidence * 100))% (not a failure probability). \(r.events) events, \(r.severeEvents) severe, host \(r.hostname), \(r.triggeredAt).")
                        .font(.callout).foregroundStyle(.secondary)
                    ProgressView(value: r.confidence).frame(maxWidth: 240)
                }.frame(maxWidth: .infinity, alignment: .leading)
            }
            Chart(r.hypotheses) { h in
                BarMark(x: .value("Evidence score", h.score), y: .value("Hypothesis", h.title)).foregroundStyle(toneForScore(h.score))
                    .annotation(position: .trailing) { Text(String(format: "%.0f", h.score)).font(.caption) }
            }
            .chartXScale(domain: 0...100).frame(height: CGFloat(40 + r.hypotheses.count * 34))
            ForEach(Array(r.hypotheses.enumerated()), id: \.element.id) { i, h in
                DisclosureGroup {
                    VStack(alignment: .leading, spacing: 8) {
                        if !h.evidence.isEmpty { Text("Evidence").font(.caption.weight(.semibold)); bullets(h.evidence) }
                        if !h.actions.isEmpty { Text("Suggested actions (advice only)").font(.caption.weight(.semibold)); bullets(h.actions) }
                    }.padding(.vertical, 4)
                } label: {
                    HStack { Text("#\(i + 1)").foregroundStyle(.secondary); Text(h.title).bold(); Spacer(); Text(String(format: "%.0f / 100", h.score)).monospacedDigit().foregroundStyle(.secondary) }
                }
            }
            if !r.topics.isEmpty {
                GroupBox("Events by topic") {
                    Chart(r.topics, id: \.name) { BarMark(x: .value("Events", $0.count), y: .value("Topic", $0.name)) }.frame(height: CGFloat(30 + r.topics.count * 26))
                }
            }
            if !r.actions.isEmpty { GroupBox("Overall suggested actions") { bullets(r.actions) } }
        }
    }

    private func load() async {
        busy = true; error = nil; note = nil; defer { busy = false }
        do { report = BlackBoxReport(json: try await model.call(AIInsightAPI.blackBox(vm: vm))) }
        catch { report = nil; self.error = error.localizedDescription }
    }
    private func freeze() async {
        guard model.isAdmin else { return }
        do { _ = try await model.call(AIInsightAPI.blackBoxTrigger(vm: vm, reason: "Machina for Mac")); note = "Capture requested for \(vm)." }
        catch { self.error = error.localizedDescription }
    }
}

// MARK: Performance autopilot

private struct InsightTypedConfirmSheet: View {
    let title: String, detail: [String], phrase: String, button: String
    let onRun: () async -> Void
    let onClose: () -> Void
    @State private var typed = ""
    @State private var busy = false
    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(title).font(.title3.weight(.semibold))
            bullets(detail)
            Text("Type \(phrase) to confirm").font(.callout)
            TextField(phrase, text: $typed).textFieldStyle(.roundedBorder).autocorrectionDisabled()
            HStack {
                Spacer()
                Button("Cancel") { onClose() }.keyboardShortcut(.cancelAction)
                Button(button, role: .destructive) { busy = true; Task { await onRun(); busy = false; onClose() } }
                    .disabled(typed != phrase || busy)
            }
        }.padding(20).frame(width: 520)
    }
}

private struct PerformancePage: View {
    @Environment(AppModel.self) private var model
    @State private var vmID = ""
    @State private var report: PerfReport?
    @State private var proposals: [AutopilotProposal] = []
    @State private var error: String?
    @State private var busy = false
    @State private var confirm: AutopilotProposal?
    @State private var result: String?

    private var vmName: String { model.fleetVMs.first { $0.id == vmID }?.name ?? report?.vmName ?? vmID }

    var body: some View {
        AIFrame(title: "Performance autopilot", subtitle: "A ranked optimisation plan for one VM, with the evidence behind each step.", caveat: AIInsightCopy.planning) {
            HStack {
                Picker("VM", selection: $vmID) {
                    Text("Choose a VM").tag("")
                    ForEach(model.fleetVMs) { Text($0.name).tag($0.id) }
                }.frame(maxWidth: 320)
                Button("Analyse") { Task { await load() } }.disabled(vmID.isEmpty || busy)
            }
            ErrorLine(text: error)
            if let result { Text(result).font(.callout).foregroundStyle(.green).textSelection(.enabled) }
            if busy { ProgressView() }
            if let r = report { planView(r) }
            if report != nil { proposalView }
        }
        .sheet(item: $confirm) { p in
            InsightTypedConfirmSheet(title: "Run \(p.label)?",
                              detail: ["Action: \(p.actionType)", "Affects one VM only: \(vmName) (\(p.vmID ?? ""))", "Risk: \(p.risk)", p.review,
                                       "This is a real change and goes through the controller as a task."],
                              phrase: AIExecute.autopilotPhrase(actionType: p.actionType, vmName: vmName), button: "Run action") {
                guard AIExecute.autopilotBlocker(role: model.role, actionType: p.actionType, vmID: p.vmID ?? "") == nil, let id = p.vmID else { return }
                do {
                    let v = try await model.call(AIExecute.autopilotExecute(actionType: p.actionType, vmID: id))
                    result = v["message"]?.text ?? "Done"; error = nil
                } catch { self.error = error.localizedDescription }
            } onClose: { confirm = nil }.environment(model)
        }
    }

    @ViewBuilder private func planView(_ r: PerfReport) -> some View {
        GroupBox("\(r.vmName) on \(r.hostname)") {
            VStack(alignment: .leading, spacing: 8) {
                Text(r.summary).textSelection(.enabled)
                Text("Health score \(Int(r.healthScore)) of 100 (model output)").font(.callout).foregroundStyle(.secondary)
                Chart(r.pressures) { p in
                    BarMark(x: .value("Pressure", p.value), y: .value("Resource", p.name)).foregroundStyle(toneForScore(p.value))
                }.chartXScale(domain: 0...100).frame(height: 150)
            }.frame(maxWidth: .infinity, alignment: .leading)
        }
        if r.actions.isEmpty {
            ContentUnavailableView("Nothing to optimise", systemImage: "checkmark.circle", description: Text("No action cleared its threshold."))
        }
        ForEach(Array(r.actions.enumerated()), id: \.element.id) { i, a in
            GroupBox {
                VStack(alignment: .leading, spacing: 6) {
                    HStack {
                        Text("\(i + 1). \(a.title)").font(.headline)
                        Spacer()
                        Text("priority \(a.priority)").font(.caption.monospacedDigit()).foregroundStyle(.secondary)
                    }
                    Grid(alignment: .leading, horizontalSpacing: 12, verticalSpacing: 3) {
                        GridRow { Text("Expected gain").foregroundStyle(.secondary); Text(a.expectedBenefit) }
                        GridRow { Text("Risk").foregroundStyle(.secondary); Text(a.risk) }
                        GridRow { Text("Reversible").foregroundStyle(.secondary); Text(a.reversible ? "yes" : "no") }
                        GridRow { Text("Needs approval").foregroundStyle(.secondary); Text(a.requiresApproval ? "yes" : "no") }
                    }.font(.callout)
                    ForEach(a.evidence) { e in
                        Text("\(e.source) \(e.metric): \(e.value.formatted(.number.precision(.fractionLength(0...2)))) \(e.unit) (threshold \(e.threshold.formatted(.number.precision(.fractionLength(0...2))))). \(e.summary)")
                            .font(.caption).foregroundStyle(.secondary).textSelection(.enabled)
                    }
                    Text("Advice only. The app does not apply this plan.").font(.caption).foregroundStyle(.secondary)
                }.frame(maxWidth: .infinity, alignment: .leading)
            }
        }
    }

    @ViewBuilder private var proposalView: some View {
        let own = proposals.filter { $0.vmID == vmID }
        GroupBox("Autopilot actions for this VM (admin, one at a time)") {
            VStack(alignment: .leading, spacing: 8) {
                if own.isEmpty { Text("The autopilot proposes nothing for this VM.").foregroundStyle(.secondary) }
                ForEach(own) { p in
                    let blocker = AIExecute.autopilotBlocker(role: model.role, actionType: p.actionType, vmID: p.vmID ?? "")
                    HStack(alignment: .top) {
                        VStack(alignment: .leading) { Text(p.label).bold(); Text(p.review).font(.caption).foregroundStyle(.secondary) }
                        Spacer()
                        Button("Preview and run…") { confirm = p }.disabled(blocker != nil).help(blocker ?? "Shows what will change first")
                    }
                }
            }.frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    private func load() async {
        busy = true; error = nil; result = nil; defer { busy = false }
        do {
            report = PerfReport(json: try await model.call(AIInsightAPI.performance(vmID: vmID)))
            proposals = AutopilotProposal.list(try await model.call(AIInsightAPI.autopilotPropose(vmID: vmID)))
        } catch { report = nil; proposals = []; self.error = error.localizedDescription }
    }
}

// MARK: Migration oracle

private struct MigrationOraclePage: View {
    @Environment(AppModel.self) private var model
    @State private var vmID = ""
    @State private var req = OracleRequest(destHostID: "")
    @State private var dirty = ""
    @State private var bandwidth = ""
    @State private var downtime = ""
    @State private var report: OracleReport?
    @State private var error: String?
    @State private var busy = false
    @State private var tele = AdaptiveTelemetry()
    @State private var decision: AdaptiveDecision?

    var body: some View {
        AIFrame(title: "Migration oracle", subtitle: "Will live pre-copy converge, and is the destination worth it.", caveat: AIInsightCopy.planning) {
            GroupBox("Predict a move") {
                VStack(alignment: .leading, spacing: 8) {
                    HStack {
                        Picker("VM", selection: $vmID) { Text("Choose a VM").tag(""); ForEach(model.fleetVMs) { Text($0.name).tag($0.id) } }
                        Picker("Destination", selection: $req.destHostID) {
                            Text("Choose a host").tag("")
                            ForEach(model.hosts.filter { $0.id != model.fleetVMs.first { $0.id == vmID }?.hostId }) { Text($0.hostname).tag($0.id) }
                        }
                    }
                    HStack {
                        TextField("Dirty rate MiB/s (optional)", text: $dirty); TextField("Bandwidth Mbit/s (optional)", text: $bandwidth)
                        TextField("Max downtime ms (optional)", text: $downtime)
                        Toggle("Live", isOn: $req.live)
                    }.textFieldStyle(.roundedBorder)
                    Button("Predict") { Task { await predict() } }.disabled(vmID.isEmpty || busy)
                    Text("The fail-closed pre-check on the migrate route stays authoritative. This page never starts a migration.").font(.caption).foregroundStyle(.secondary)
                }
            }
            ErrorLine(text: error)
            if busy { ProgressView() }
            if let r = report { verdictView(r) }
            decisionBox
        }
    }

    @ViewBuilder private func verdictView(_ r: OracleReport) -> some View {
        let color: Color = r.verdict == .converges && r.precheckOK ? .green : r.verdict == .marginal ? .orange : r.verdict == .unknown ? .gray : .red
        GroupBox {
            VStack(alignment: .leading, spacing: 8) {
                HStack {
                    Image(systemName: r.looksSafe ? "checkmark.seal.fill" : "exclamationmark.triangle.fill").foregroundStyle(color).font(.title)
                    VStack(alignment: .leading) {
                        Text(r.verdict.label).font(.title2.weight(.semibold)).foregroundStyle(color)
                        Text("\(r.vmName): \(r.source) to \(r.destination), \(r.live ? "live" : "offline"). Server wording: \(r.convergence). Pre-check \(r.precheckOK ? "passed" : "FAILED").")
                            .font(.callout).foregroundStyle(.secondary)
                    }
                }
                Grid(alignment: .leading, horizontalSpacing: 16, verticalSpacing: 3) {
                    GridRow { Text("Convergence score").foregroundStyle(.secondary); Text(String(format: "%.0f", r.score)) }
                    GridRow { Text("Estimated total").foregroundStyle(.secondary); Text(String(format: "%.0f s, %.1f GiB moved", r.totalSeconds, r.transferredGiB)) }
                    GridRow { Text("Estimated downtime").foregroundStyle(.secondary); Text(String(format: "%.0f ms", r.downtimeMs)) }
                    GridRow { Text("Dirty rate vs bandwidth").foregroundStyle(.secondary); Text(String(format: "%.0f MiB/s against %.0f MiB/s (needs at least %.0f Mbit/s)", r.dirtyRate, r.bandwidth, r.minimumBandwidthMbps)) }
                    GridRow { Text("Destination benefit").foregroundStyle(.secondary); Text(String(format: "%.0f, CPU %+.0f%%, %d MiB headroom after", r.benefit, r.cpuImprovementPct, r.headroomMiB)) }
                }.font(.callout)
            }.frame(maxWidth: .infinity, alignment: .leading)
        }
        if !r.rounds.isEmpty {
            GroupBox("Pre-copy rounds: memory still to send (MiB)") {
                Chart(r.rounds) { LineMark(x: .value("Round", $0.id), y: .value("Remaining MiB", $0.remainingMiB)); PointMark(x: .value("Round", $0.id), y: .value("Remaining MiB", $0.remainingMiB)) }
                    .frame(height: 160)
            }
        }
        if !r.risks.isEmpty { GroupBox("Risks") { bullets(r.risks) } }
        if !r.evidence.isEmpty { GroupBox("Evidence") { bullets(r.evidence) } }
        if !r.recommendations.isEmpty { GroupBox("Recommendations (advice only)") { bullets(r.recommendations) } }
    }

    private var decisionBox: some View {
        GroupBox("Adaptive decision for a running migration") {
            VStack(alignment: .leading, spacing: 8) {
                Text("Enter one telemetry sample. The engine answers with the next tuning step; nothing is applied.").font(.caption).foregroundStyle(.secondary)
                Grid(alignment: .leading, horizontalSpacing: 12, verticalSpacing: 4) {
                    GridRow { num("Remaining MiB", $tele.remainingMiB); num("Dirty MiB/s", $tele.dirtyRate); num("Effective MiB/s", $tele.bandwidth); num("Current limit MiB/s", $tele.currentBandwidth) }
                    GridRow { num("Elapsed s", $tele.elapsedSecs); num("Iteration", $tele.iteration); num("Transferred MiB", $tele.transferredMiB); num("Stalled rounds", $tele.stalledRounds) }
                    GridRow { num("Guest CPU %", $tele.guestCpu); num("Source CPU %", $tele.sourceCpu); num("Dest CPU %", $tele.destCpu); num("Packet loss %", $tele.packetLoss) }
                    GridRow { num("vCPU throttle %", $tele.throttle); Toggle("Compression on", isOn: $tele.compression); Toggle("Post-copy active", isOn: $tele.postcopy) }
                }
                HStack {
                    Button("Decide") { Task { await decide() } }
                    if let msg = tele.validate() { Text(msg).font(.caption).foregroundStyle(.orange) }
                }
                if let d = decision {
                    Text(d.title).font(.headline)
                    Text(d.reason).textSelection(.enabled)
                    Text("Bandwidth \(Int(d.bandwidth)) MiB/s, target downtime \(Int(d.targetDowntimeMs)) ms, throttle \(Int(d.throttlePct))%, compression \(d.compression ? "on" : "off"), post-copy \(d.postcopy ? "on" : "off"). Convergence ratio \(d.convergenceRatio.map { String(format: "%.2f", $0) } ?? "unbounded"), estimated downtime \(d.estimatedDowntimeMs.map { String(format: "%.0f ms", $0) } ?? "unbounded"), risk \(String(format: "%.2f", d.risk)).")
                        .font(.callout).foregroundStyle(.secondary)
                }
            }.frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    private func num(_ label: String, _ v: Binding<Double>) -> some View {
        VStack(alignment: .leading, spacing: 1) { Text(label).font(.caption).foregroundStyle(.secondary); TextField(label, value: v, format: .number).textFieldStyle(.roundedBorder).frame(width: 110) }
    }

    private func predict() async {
        req.dirtyRateMiBs = Double(dirty); req.bandwidthMbps = Double(bandwidth); req.maxDowntimeMs = Double(downtime)
        if let m = req.validate() { error = m; return }
        busy = true; error = nil; defer { busy = false }
        do { report = OracleReport(json: try await model.call(AIInsightAPI.oracle(vmID: vmID, req))) } catch { report = nil; self.error = error.localizedDescription }
    }
    private func decide() async {
        if let m = tele.validate() { error = m; return }
        error = nil
        do { decision = AdaptiveDecision(json: try await model.call(AIInsightAPI.decision(tele))) } catch { decision = nil; self.error = error.localizedDescription }
    }
}

// MARK: Fleet

private struct FleetInsightPage: View {
    @Environment(AppModel.self) private var model
    @State private var summary: FleetSummaryInfo?
    @State private var heat: FleetHeat?
    @State private var power: PowerReport?
    @State private var move: RebalanceMove?
    @State private var proposalNote: String?
    @State private var error: String?
    @State private var confirm: RebalanceMove?
    @State private var result: String?

    var body: some View {
        AIFrame(title: "Fleet insight", subtitle: "Cost and headroom per cluster, host heat, power savings and the next rebalance move.", caveat: AIInsightCopy.unverified) {
            HStack { Button("Refresh") { Task { await load() } }; Spacer() }
            ErrorLine(text: error)
            if let result { Text(result).font(.callout).foregroundStyle(.green).textSelection(.enabled) }
            if let s = summary {
                GroupBox("Clusters: \(s.vmCount) VMs, about \(s.aggregateUSD.formatted(.currency(code: "USD")))/month, \(s.reachablePeers) of \(s.peers) peers reachable") {
                    Chart(s.clusters) { c in
                        BarMark(x: .value("USD per month", c.monthlyUSD), y: .value("Cluster", c.label)).foregroundStyle(c.reachable ? Color.accentColor : .gray)
                    }.frame(height: CGFloat(40 + s.clusters.count * 30))
                    ForEach(s.clusters) { c in
                        Text("\(c.label): \(c.vmCount) VMs, \(c.headroomMiB) MiB memory headroom, security risk \(c.risk)\(c.reachable ? "" : " (unreachable)")").font(.caption).foregroundStyle(.secondary)
                    }
                }
            }
            if let h = heat {
                GroupBox("Host heat (hottest first)") {
                    Chart {
                        ForEach(h.hosts) { c in
                            BarMark(x: .value("Percent", c.cpu), y: .value("Host", c.hostname)).position(by: .value("Metric", "CPU")).foregroundStyle(by: .value("Metric", "CPU"))
                            BarMark(x: .value("Percent", c.memory), y: .value("Host", c.hostname)).position(by: .value("Metric", "Memory")).foregroundStyle(by: .value("Metric", "Memory"))
                        }
                    }.chartXScale(domain: 0...100).frame(height: CGFloat(60 + h.hosts.count * 46))
                    if !h.hotspots.isEmpty { Text("Hotspots: \(h.hotspots.joined(separator: ", "))").font(.callout) }
                    if !h.wasteful.isEmpty { Text("Power waste: \(h.wasteful.joined(separator: ", "))").font(.callout) }
                }
            }
            if let p = power {
                GroupBox("Power optimisation: \(p.totalUSD.formatted(.currency(code: "USD")))/month possible (advice only)") {
                    Text(p.summary).font(.callout).foregroundStyle(.secondary)
                    ForEach(p.items) { i in
                        HStack(alignment: .top) { Text(i.host).bold().frame(width: 140, alignment: .leading); Text("\(i.action): \(i.reason)"); Spacer(); Text(i.savingsUSD.formatted(.currency(code: "USD"))).monospacedDigit() }.font(.callout)
                    }
                }
            }
            rebalanceBox
        }
        .task { await load() }
        .sheet(item: $confirm) { m in
            InsightTypedConfirmSheet(title: "Move \(m.vmName)?",
                              detail: ["VM: \(m.vmName) (\(m.vmID))", "From host \(m.from) to host \(m.to)", m.reason,
                                       "This starts a real migration task for this one VM. The controller picks the move, so the app re-checks that it is still this VM first."],
                              phrase: AIExecute.rebalancePhrase(vmName: m.vmName), button: "Start migration") {
                guard AIExecute.rebalanceBlocker(role: model.role) == nil else { return }
                do {
                    let fresh = AIExecute.rebalanceMoves(try await model.call(AIExecute.rebalanceExecute(dryRun: true)))
                    guard AIExecute.rebalanceStillMatches(confirmed: m.vmID, fresh: fresh) else {
                        error = "The proposed move changed while you were confirming. Nothing was started; review it again."; return
                    }
                    let v = try await model.call(AIExecute.rebalanceExecute(dryRun: false))
                    result = "\(v["summary"]?.text ?? "Started") Tasks: \(AIExecute.taskIDs(v).joined(separator: ", "))"; error = nil
                    await load()
                } catch { self.error = error.localizedDescription }
            } onClose: { confirm = nil }.environment(model)
        }
    }

    private var rebalanceBox: some View {
        GroupBox("Next rebalance move (one VM at a time)") {
            VStack(alignment: .leading, spacing: 6) {
                if let m = move {
                    Text("\(m.vmName): host \(m.from) to host \(m.to)").bold()
                    Text(m.reason).font(.callout).foregroundStyle(.secondary)
                    let blocker = AIExecute.rebalanceBlocker(role: model.role)
                    Button("Preview and move…") { confirm = m }.disabled(blocker != nil).help(blocker ?? "Shows what will change first")
                } else { Text(proposalNote ?? "The placement engine proposes no move.").foregroundStyle(.secondary) }
            }.frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    private func load() async {
        error = nil
        do { summary = FleetSummaryInfo(json: try await model.call(AIInsightAPI.fleetSummary)) } catch { self.error = error.localizedDescription }
        do { heat = FleetHeat(json: try await model.call(AIInsightAPI.heatmap)) } catch { self.error = error.localizedDescription }
        do { power = PowerReport(json: try await model.call(AIInsightAPI.power)) } catch { self.error = error.localizedDescription }
        do {
            let v = try await model.call(AIInsightAPI.rebalancePropose(maxMoves: 1))
            move = AIExecute.rebalanceMoves(v).first; proposalNote = v["summary"]?.text
        } catch { self.error = error.localizedDescription }
    }
}

// MARK: Noisy neighbours

private struct NoisyGraphPage: View {
    @Environment(AppModel.self) private var model
    @State private var report: NoisyReport?
    @State private var error: String?
    @State private var selected: NoisyEdge?

    var body: some View {
        AIFrame(title: "Noisy neighbour graph", subtitle: "Aggressor VMs on the left, the VMs they slow down on the right. Thicker and redder means a stronger edge.", caveat: AIInsightCopy.planning) {
            HStack { Button("Refresh") { Task { await load() } }; Spacer() }
            ErrorLine(text: error)
            if let r = report {
                Text(r.summary).foregroundStyle(.secondary)
                if r.edges.isEmpty {
                    ContentUnavailableView("No interference found", systemImage: "waveform.path.ecg")
                } else {
                    NoisyCanvas(report: r, selected: $selected).frame(height: CGFloat(max(180, min(520, r.nodes.count * 44))))
                    if let e = selected {
                        GroupBox("\(e.aggressor) to \(e.victim) on \(e.hostname)") {
                            VStack(alignment: .leading, spacing: 4) {
                                Text("Score \(Int(e.score)), confidence \(Int(e.confidence * 100))%, estimated latency impact \(String(format: "%.0f", e.impactPct))%, resources: \(e.resources.joined(separator: ", "))").font(.callout)
                                bullets(e.evidence); bullets(e.recommendations)
                            }.frame(maxWidth: .infinity, alignment: .leading)
                        }
                    } else { Text("Click an edge in the list below for its evidence.").font(.caption).foregroundStyle(.secondary) }
                    ForEach(r.edges) { e in
                        Button { selected = e } label: {
                            HStack { Text("\(e.aggressor) to \(e.victim)").bold(); Spacer(); Text("score \(Int(e.score))").monospacedDigit().foregroundStyle(.secondary) }
                        }.buttonStyle(.plain)
                    }
                }
            }
        }
        .task { await load() }
    }
    private func load() async {
        error = nil
        do { report = NoisyReport(json: try await model.call(AIInsightAPI.noisy)); selected = nil } catch { self.error = error.localizedDescription }
    }
}

private struct NoisyCanvas: View {
    let report: NoisyReport
    @Binding var selected: NoisyEdge?
    var body: some View {
        let nodes = report.nodes
        let byName = Dictionary(uniqueKeysWithValues: nodes.map { ($0.name, $0) })
        let top = max(report.edges.first?.score ?? 1, 1)
        Canvas { ctx, size in
            let pad: CGFloat = 130
            func point(_ n: NoisyNode) -> CGPoint { CGPoint(x: pad + n.x * (size.width - 2 * pad), y: 20 + n.y * (size.height - 40)) }
            for e in report.edges {
                guard let a = byName[e.aggressor], let v = byName[e.victim] else { continue }
                var p = Path(); let pa = point(a), pv = point(v)
                p.move(to: pa)
                p.addCurve(to: pv, control1: CGPoint(x: (pa.x + pv.x) / 2, y: pa.y), control2: CGPoint(x: (pa.x + pv.x) / 2, y: pv.y))
                let w = e.score / top
                let hot = e.id == selected?.id
                ctx.stroke(p, with: .color(hot ? .accentColor : Color(hue: 0.08 * (1 - w), saturation: 0.8, brightness: 0.9).opacity(0.75)), lineWidth: 1 + 6 * w)
            }
            for n in nodes {
                let c = point(n)
                ctx.fill(Path(ellipseIn: CGRect(x: c.x - 6, y: c.y - 6, width: 12, height: 12)), with: .color(n.isAggressor ? .red : .blue))
                let t = Text(n.name).font(.caption)
                ctx.draw(t, at: CGPoint(x: c.x + (n.x == 0 ? -12 : 12), y: c.y), anchor: n.x == 0 ? .trailing : .leading)
            }
        }
        .accessibilityLabel("Graph of \(report.edges.count) aggressor to victim edges")
    }
}
