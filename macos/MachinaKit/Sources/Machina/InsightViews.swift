import SwiftUI
import Charts
import MachinaKit

extension Tone {
    var color: Color {
        switch self { case .ok: .green; case .warn: .orange; case .bad: .red; case .idle: .gray }
    }
}

/// Five-step heat ramp, cold to hot.
private func heatColor(_ level: Int?) -> Color {
    guard let level else { return .gray.opacity(0.15) }
    return [Color.green.opacity(0.35), .green.opacity(0.7), .yellow.opacity(0.85), .orange, .red][min(max(level, 0), 4)]
}

private func pct(_ v: Double?) -> String { v.map { String(format: "%.0f%%", $0) } ?? "–" }

/// Shared page chrome: title, subtitle and the platform error banner.
private struct InsightPageFrame<Content: View>: View {
    @Environment(AppModel.self) private var model
    let title: String, subtitle: String
    @ViewBuilder var content: Content
    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                VStack(alignment: .leading, spacing: 2) {
                    Text(title).font(.largeTitle.weight(.semibold))
                    Text(subtitle).foregroundStyle(.secondary)
                }
                PlatformErrorBanner(text: model.platformError)
                content
            }.padding(24).frame(maxWidth: .infinity, alignment: .leading)
        }
    }
}

struct InsightRouter: View {
    let page: InsightPage
    var body: some View {
        switch page {
        case .topology: TopologyView()
        case .grid: VMGridView()
        case .heatmap: CapacityHeatmapView()
        case .timeline: ActivityTimelineView()
        }
    }
}

private struct ToneLegend: View {
    var body: some View {
        HStack(spacing: 12) {
            ForEach([("Running", Tone.ok), ("Paused", .warn), ("Crashed", .bad), ("Stopped", .idle)], id: \.0) { n, t in
                Label(n, systemImage: "circle.fill").foregroundStyle(t.color).font(.caption).labelStyle(.titleAndIcon)
            }
        }
    }
}

// MARK: Topology map

/// Controller at the top, hosts below it, each with its VMs as coloured chips.
struct TopologyView: View {
    @Environment(AppModel.self) private var model
    private static let cardWidth: CGFloat = 290, gap: CGFloat = 16

    private var groups: [HostGroup] {
        let vms = model.fleetVMs.isEmpty ? model.vms.map { PlatformVM($0) } : model.fleetVMs
        return Topology.group(hosts: model.hosts, vms: vms, unassignedName: model.hosts.isEmpty ? "This host" : "Unassigned")
    }

    var body: some View {
        InsightPageFrame(title: "Topology map", subtitle: "Hosts and the VMs placed on them. Click a VM to open it.") {
            let g = groups
            ToneLegend()
            if g.isEmpty {
                if model.platformLoaded { ContentUnavailableView("Nothing to map", systemImage: "point.3.connected.trianglepath.dotted") } else { ProgressView() }
            } else {
                ScrollView(.horizontal) {
                    VStack(spacing: 0) {
                        hub(g)
                        fan(count: g.count)
                        HStack(alignment: .top, spacing: Self.gap) { ForEach(g) { HostCard(group: $0).frame(width: Self.cardWidth) } }
                    }.padding(.bottom, 8)
                }
            }
        }
    }

    private func hub(_ g: [HostGroup]) -> some View {
        let total = g.reduce(0) { $0 + $1.vms.count }, running = g.reduce(0) { $0 + $1.running }
        let width = max(CGFloat(g.count) * (Self.cardWidth + Self.gap) - Self.gap, 240)
        return HStack {
            Spacer()
            VStack(spacing: 2) {
                Label("Fleet", systemImage: "server.rack").font(.headline)
                Text("\(g.count) hosts · \(running)/\(total) VMs running").font(.caption).foregroundStyle(.secondary)
            }
            .padding(.horizontal, 16).padding(.vertical, 8)
            .background(.tint.opacity(0.15), in: .rect(cornerRadius: 12))
            Spacer()
        }.frame(width: width)
    }

    /// Curves from the hub's centre to the middle of each card column.
    private func fan(count: Int) -> some View {
        let width = max(CGFloat(count) * (Self.cardWidth + Self.gap) - Self.gap, 240)
        return Canvas { ctx, size in
            let origin = CGPoint(x: size.width / 2, y: 0)
            for i in 0..<count {
                let x = Self.cardWidth / 2 + CGFloat(i) * (Self.cardWidth + Self.gap)
                var p = Path(); p.move(to: origin)
                p.addCurve(to: CGPoint(x: x, y: size.height), control1: CGPoint(x: origin.x, y: size.height * 0.6), control2: CGPoint(x: x, y: size.height * 0.4))
                ctx.stroke(p, with: .color(.secondary.opacity(0.5)), lineWidth: 1.5)
            }
        }.frame(width: width, height: 44)
    }
}

private struct HostCard: View {
    @Environment(AppModel.self) private var model
    let group: HostGroup
    private let cols = [GridItem(.adaptive(minimum: 78, maximum: 120), spacing: 6)]

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(spacing: 8) {
                Circle().fill(group.tone.color).frame(width: 10, height: 10)
                Text(group.name).font(.headline).lineLimit(1)
                Spacer()
                if group.host?.maintenanceMode == true { Text("maintenance").font(.caption2).padding(.horizontal, 5).background(.orange.opacity(0.25), in: .capsule) }
            }
            if let h = group.host {
                HStack(spacing: 14) {
                    Gauge(label: "CPU", pct: h.cpuPercent)
                    Gauge(label: "Mem", pct: h.memoryPercent)
                    VStack(alignment: .leading, spacing: 2) {
                        Text("\(group.running)/\(group.vms.count)").font(.title3.weight(.semibold)).monospacedDigit()
                        Text("VMs running").font(.caption2).foregroundStyle(.secondary)
                    }
                }
            }
            if group.vms.isEmpty {
                Text("No VMs").font(.caption).foregroundStyle(.secondary)
            } else {
                LazyVGrid(columns: cols, spacing: 6) {
                    ForEach(group.vms) { vm in
                        Button { if model.vms.contains(where: { $0.name == vm.name }) { model.selection = .vm(vm.name) } } label: {
                            Text(vm.name).font(.caption).lineLimit(1).truncationMode(.middle)
                                .padding(.horizontal, 6).padding(.vertical, 5).frame(maxWidth: .infinity)
                                .background(vm.tone.color.opacity(vm.tone == .idle ? 0.18 : 0.3), in: .rect(cornerRadius: 6))
                                .overlay(RoundedRectangle(cornerRadius: 6).stroke(vm.tone.color.opacity(0.7), lineWidth: 1))
                        }.buttonStyle(.plain)
                        .help("\(vm.name): \(vm.observedState), \(vm.vcpus) vCPU, \(vm.memoryMib) MiB")
                    }
                }
            }
        }
        .padding(12)
        .background(.quaternary.opacity(0.4), in: .rect(cornerRadius: 12))
        .overlay(RoundedRectangle(cornerRadius: 12).stroke(group.tone.color.opacity(0.5), lineWidth: 1))
    }
}

/// Small ring gauge for a utilisation percent.
private struct Gauge: View {
    let label: String
    let pct: Double?
    var body: some View {
        ZStack {
            Circle().stroke(.quaternary, lineWidth: 5)
            Circle().trim(from: 0, to: min(max((pct ?? 0) / 100, 0), 1)).stroke(Tone.load(pct).color, style: .init(lineWidth: 5, lineCap: .round))
                .rotationEffect(.degrees(-90))
            VStack(spacing: 0) {
                Text(pct.map { String(format: "%.0f", $0) } ?? "–").font(.caption.weight(.semibold)).monospacedDigit()
                Text(label).font(.system(size: 8)).foregroundStyle(.secondary)
            }
        }.frame(width: 46, height: 46)
    }
}

// MARK: VM grid with sparklines

struct VMGridView: View {
    @Environment(AppModel.self) private var model
    @State private var order: Spark.Order = .state
    @State private var search = ""

    private var rows: [VmInfo] {
        let cpu = Dictionary(uniqueKeysWithValues: model.vms.compactMap { vm in Spark.cpu(model.history, vm: vm.name, count: 1).last.map { (vm.name, $0) } })
        let list = search.isEmpty ? model.vms : model.vms.filter { $0.name.localizedCaseInsensitiveContains(search) }
        return Spark.order(list, by: order, cpu: cpu)
    }

    var body: some View {
        InsightPageFrame(title: "VM grid", subtitle: "Every VM on this host with its recent CPU and memory. Click a tile to open it.") {
            HStack {
                TextField("Filter", text: $search).textFieldStyle(.roundedBorder).frame(maxWidth: 220)
                Picker("Sort", selection: $order) { ForEach(Spark.Order.allCases, id: \.self) { Text($0.rawValue).tag($0) } }
                    .pickerStyle(.segmented).frame(maxWidth: 240)
                Spacer()
                ToneLegend()
            }
            LazyVGrid(columns: [GridItem(.adaptive(minimum: 200, maximum: 280), spacing: 12)], spacing: 12) {
                ForEach(rows) { vm in
                    VMTile(vm: vm, cpu: Spark.cpu(model.history, vm: vm.name), mem: Spark.memoryPercent(model.history, vm: vm.name))
                        .onTapGesture { model.selection = .vm(vm.name) }
                }
            }
            if model.vms.isEmpty { ContentUnavailableView("No VMs", systemImage: "square.grid.3x3") }
        }
    }
}

private struct VMTile: View {
    let vm: VmInfo
    let cpu: [Double], mem: [Double]
    var body: some View {
        let tone = Tone.vm(vm.state)
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 6) {
                Circle().fill(tone.color).frame(width: 8, height: 8)
                Text(vm.name).font(.headline).lineLimit(1).truncationMode(.middle)
                Spacer()
                Text("\(vm.vcpus) vCPU · \(vm.memoryMb) MiB").font(.caption2).foregroundStyle(.secondary)
            }
            SparkRow(title: "CPU", values: cpu, color: .blue, cap: 100)
            SparkRow(title: "Mem", values: mem, color: .purple, cap: 100)
        }
        .padding(12)
        .background(.quaternary.opacity(0.4), in: .rect(cornerRadius: 10))
        .overlay(RoundedRectangle(cornerRadius: 10).stroke(tone.color.opacity(tone == .idle ? 0.15 : 0.5), lineWidth: 1))
        .opacity(vm.isRunning ? 1 : 0.7)
        .contentShape(.rect)
    }
}

private struct SparkRow: View {
    let title: String
    let values: [Double]
    let color: Color
    let cap: Double
    var body: some View {
        HStack(spacing: 8) {
            Text(title).font(.caption2).foregroundStyle(.secondary).frame(width: 26, alignment: .leading)
            if values.count < 2 {
                Text("no samples").font(.caption2).foregroundStyle(.tertiary).frame(maxWidth: .infinity, minHeight: 26, alignment: .leading)
            } else {
                Chart(Array(values.enumerated()), id: \.offset) { i, v in
                    AreaMark(x: .value("t", i), y: .value(title, v)).foregroundStyle(color.opacity(0.18)).interpolationMethod(.monotone)
                    LineMark(x: .value("t", i), y: .value(title, v)).foregroundStyle(color).interpolationMethod(.monotone)
                }
                .chartXAxis(.hidden).chartYAxis(.hidden).chartYScale(domain: 0...cap).frame(height: 26)
            }
            Text(values.last.map { String(format: "%.0f%%", $0) } ?? "–").font(.caption.weight(.medium)).monospacedDigit().frame(width: 38, alignment: .trailing)
        }
    }
}

// MARK: Capacity heatmap

struct CapacityHeatmapView: View {
    @Environment(AppModel.self) private var model
    @State private var sizeGiB = 2

    private var groups: [HostGroup] {
        let vms = model.fleetVMs.isEmpty ? model.vms.map { PlatformVM($0) } : model.fleetVMs
        return Topology.group(hosts: model.hosts, vms: vms).filter { $0.host != nil }
    }

    var body: some View {
        InsightPageFrame(title: "Capacity heatmap", subtitle: "Where the fleet is hot and where a new VM would fit.") {
            HStack {
                Picker("New VM size", selection: $sizeGiB) { ForEach([1, 2, 4, 8, 16], id: \.self) { Text("\($0) GiB").tag($0) } }.frame(maxWidth: 220)
                Spacer()
                HStack(spacing: 2) {
                    Text("cool").font(.caption2).foregroundStyle(.secondary)
                    ForEach(0..<5, id: \.self) { RoundedRectangle(cornerRadius: 3).fill(heatColor($0)).frame(width: 18, height: 12) }
                    Text("hot").font(.caption2).foregroundStyle(.secondary)
                }
            }
            let g = groups
            if g.isEmpty {
                if model.platformLoaded { Text("The controller reports no hosts.").foregroundStyle(.secondary) } else { ProgressView() }
            } else {
                Grid(horizontalSpacing: 6, verticalSpacing: 6) {
                    GridRow {
                        Text("Host").font(.caption).foregroundStyle(.secondary).gridColumnAlignment(.leading)
                        ForEach(["CPU", "Memory used", "Memory committed"], id: \.self) { Text($0).font(.caption).foregroundStyle(.secondary) }
                        Text("Fits").font(.caption).foregroundStyle(.secondary)
                    }
                    ForEach(g) { grp in
                        let h = grp.host!
                        GridRow {
                            HStack(spacing: 6) { Circle().fill(grp.tone.color).frame(width: 8, height: 8); Text(grp.name).lineLimit(1) }
                            HeatCell(value: h.isOnline ? h.cpuPercent : nil)
                            HeatCell(value: h.isOnline ? h.memoryPercent : nil)
                            HeatCell(value: Heat.memoryCommitPercent(grp))
                            Text("\(Heat.headroom(grp, sizeMib: sizeGiB * 1024)) × \(sizeGiB) GiB").monospacedDigit().font(.callout)
                        }
                    }
                }
            }
            LocalHistoryStrip()
        }
    }
}

private struct HeatCell: View {
    let value: Double?
    var body: some View {
        Text(pct(value)).font(.callout.weight(.medium)).monospacedDigit()
            .frame(maxWidth: .infinity, minHeight: 34)
            .background(heatColor(Heat.level(value)), in: .rect(cornerRadius: 6))
            .foregroundStyle(Heat.level(value).map { $0 >= 2 ? Color.black.opacity(0.8) : .primary } ?? .secondary)
    }
}

/// Time-versus-metric heatmap for this host built from its metrics history.
private struct LocalHistoryStrip: View {
    @Environment(AppModel.self) private var model
    private struct Cell: Identifiable { var id: String { "\(metric)\(col)" }; var metric: String; var col: Int; var value: Double }

    private var cells: [Cell] {
        let pts = model.history.sorted { $0.timestampMs < $1.timestampMs }
        let rows: [(String, [Double])] = [("CPU", pts.compactMap(\.hostCpuPercent)), ("Memory", pts.compactMap(\.hostMemoryPercent)), ("Disk", pts.compactMap(\.hostDiskPercent))]
        return rows.flatMap { name, vals in Heat.bucketed(vals, columns: 48).enumerated().map { Cell(metric: name, col: $0.offset, value: $0.element) } }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("This host over time").font(.headline)
            if cells.isEmpty {
                Text("No history yet").foregroundStyle(.secondary)
            } else {
                Chart(cells) { c in
                    RectangleMark(x: .value("Time", c.col), y: .value("Metric", c.metric), width: .ratio(1), height: .ratio(0.9))
                        .foregroundStyle(heatColor(Heat.level(c.value)))
                }
                .chartXAxis(.hidden).frame(height: 110)
                Text("Oldest on the left. Each cell is the average of several samples.").font(.caption).foregroundStyle(.secondary)
            }
        }
        .padding(14).background(.quaternary.opacity(0.4), in: .rect(cornerRadius: 10))
    }
}

// MARK: What changed

struct ActivityTimelineView: View {
    @Environment(AppModel.self) private var model
    @State private var filter = ""

    private static let timeFormat: DateFormatter = { let f = DateFormatter(); f.timeStyle = .medium; f.dateStyle = .none; return f }()

    var body: some View {
        InsightPageFrame(title: "What changed", subtitle: "VM state changes as they arrive on the live stream since you signed in.") {
            TimelineView(.periodic(from: .now, by: 30)) { ctx in
                let buckets = ActivityLog.buckets(model.activity, slot: 300, slots: 12, now: ctx.date)
                VStack(alignment: .leading, spacing: 6) {
                    Text("Last hour").font(.headline)
                    if buckets.isEmpty {
                        Text("No changes in the last hour").font(.callout).foregroundStyle(.secondary).frame(maxWidth: .infinity, minHeight: 60)
                    } else {
                        Chart(buckets) { b in
                            BarMark(x: .value("Time", b.start, unit: .minute, calendar: .current), y: .value("Changes", b.count))
                                .foregroundStyle(b.tone.color)
                        }
                        .chartXScale(domain: ctx.date.addingTimeInterval(-3600)...ctx.date.addingTimeInterval(300))
                        .frame(height: 110)
                    }
                }
                .padding(14).background(.quaternary.opacity(0.4), in: .rect(cornerRadius: 10))
            }
            HStack {
                TextField("Filter by VM", text: $filter).textFieldStyle(.roundedBorder).frame(maxWidth: 240)
                Spacer()
                Text("\(model.activity.count) events").font(.caption).foregroundStyle(.secondary)
            }
            let rows = ActivityLog.filter(model.activity, vm: filter)
            if rows.isEmpty {
                ContentUnavailableView(model.activity.isEmpty ? "Nothing has changed yet" : "No matches", systemImage: "clock.arrow.circlepath")
            } else {
                LazyVStack(alignment: .leading, spacing: 0) {
                    ForEach(rows) { e in
                        HStack(spacing: 10) {
                            Circle().fill(e.tone.color).frame(width: 9, height: 9)
                            Text(Self.timeFormat.string(from: e.date)).font(.caption).foregroundStyle(.secondary).monospacedDigit().frame(width: 76, alignment: .leading)
                            Button(e.vm) { if model.vms.contains(where: { $0.name == e.vm }) { model.selection = .vm(e.vm) } }.buttonStyle(.link)
                            Text(e.summary).foregroundStyle(.secondary)
                            Spacer()
                        }.padding(.vertical, 6)
                        Divider()
                    }
                }
            }
        }
    }
}
