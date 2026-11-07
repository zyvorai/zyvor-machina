import SwiftUI
import MachinaKit

private func pct(_ v: Double?) -> String { v.map { String(format: "%.0f%%", $0) } ?? "–" }

private func stateColor(_ s: String) -> Color {
    switch s.lowercased() {
    case "online", "completed", "succeeded", "success", "info": .green
    case "running", "pending", "queued", "in_progress", "warning": .orange
    case "offline", "failed", "error", "critical", "cancelled": .red
    default: .gray
    }
}

struct PlatformErrorBanner: View {
    let text: String?
    var body: some View {
        if let text {
            Label(text, systemImage: "exclamationmark.triangle.fill")
                .font(.callout).foregroundStyle(.orange).padding(10)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(.orange.opacity(0.12), in: .rect(cornerRadius: 8))
        }
    }
}

struct MetricTile: View {
    let title: String, value: String
    var fraction: Double?
    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(title).font(.caption).foregroundStyle(.secondary)
            Text(value).font(.title2.weight(.semibold)).monospacedDigit()
            if let f = fraction { ProgressView(value: min(max(f, 0), 1)).tint(f > 0.9 ? .red : f > 0.75 ? .orange : .accentColor) }
        }
        .padding(12).frame(maxWidth: .infinity, alignment: .leading)
        .background(.quaternary.opacity(0.5), in: .rect(cornerRadius: 10))
    }
}

struct FleetView: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 20) {
                Text("Fleet").font(.largeTitle.weight(.semibold))
                PlatformErrorBanner(text: model.platformError)
                if let m = model.metrics {
                    Text("This host").font(.headline)
                    LazyVGrid(columns: [GridItem(.adaptive(minimum: 150), spacing: 12)], spacing: 12) {
                        MetricTile(title: "CPU", value: pct(m.hostCpuPercent), fraction: (m.hostCpuPercent ?? 0) / 100)
                        MetricTile(title: "Memory", value: pct(m.hostMemoryPercent), fraction: (m.hostMemoryPercent ?? 0) / 100)
                        MetricTile(title: "Disk", value: pct(m.hostDiskPercent), fraction: (m.hostDiskPercent ?? 0) / 100)
                        MetricTile(title: "Load (1m)", value: m.load1.map { String(format: "%.2f", $0) } ?? "–")
                        MetricTile(title: "VMs running", value: "\(m.vmsRunning ?? 0) / \(m.vmCount ?? 0)")
                        MetricTile(title: "Capacity", value: (m.capacity?.label ?? "–").capitalized,
                                   fraction: m.capacity?.score.map { $0 / 100 })
                    }
                }
                Text("Hosts (\(model.hosts.count))").font(.headline)
                if model.hosts.isEmpty {
                    if model.platformLoaded { Text("No hosts reported by the controller.").foregroundStyle(.secondary) }
                    else { ProgressView() }
                } else {
                    Table(model.hosts) {
                        TableColumn("Host") { h in
                            HStack(spacing: 6) {
                                Circle().fill(stateColor(h.state)).frame(width: 8, height: 8)
                                Text(h.hostname)
                                if h.maintenanceMode == true { Text("maintenance").font(.caption2).padding(.horizontal, 5)
                                    .background(.orange.opacity(0.25), in: .capsule) }
                                if h.fenced == true { Text("fenced").font(.caption2).padding(.horizontal, 5)
                                    .background(.red.opacity(0.25), in: .capsule) }
                            }
                        }
                        TableColumn("State") { Text($0.state.capitalized).foregroundStyle(stateColor($0.state)) }
                        TableColumn("Address") { Text($0.address ?? "–").textSelection(.enabled) }
                        TableColumn("VMs") { Text("\($0.vmCount ?? 0)").monospacedDigit() }
                        TableColumn("CPU") { Text(pct($0.cpuPercent)).monospacedDigit() }
                        TableColumn("Memory") { Text(pct($0.memoryPercent)).monospacedDigit() }
                    }
                    .frame(minHeight: CGFloat(60 + model.hosts.count * 28))
                }
            }.padding(24)
        }
    }
}

struct TasksView: View {
    @Environment(AppModel.self) private var model
    @State private var selection: PlatformTask.ID?
    @State private var showActiveOnly = false

    private var rows: [PlatformTask] { showActiveOnly ? model.tasks.filter(\.isActive) : model.tasks }
    private var selected: PlatformTask? { model.tasks.first { $0.id == selection } }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Text("Tasks").font(.largeTitle.weight(.semibold))
                Spacer()
                Toggle("Active only", isOn: $showActiveOnly).toggleStyle(.checkbox)
                Button("Cancel") { if let t = selected { Task { await model.taskAction(cancel: true, t) } } }
                    .disabled(selected?.isActive != true)
                Button("Retry") { if let t = selected { Task { await model.taskAction(cancel: false, t) } } }
                    .disabled(selected?.isFailed != true)
            }
            PlatformErrorBanner(text: model.platformError)
            if rows.isEmpty {
                if model.platformLoaded { ContentUnavailableView("No tasks", systemImage: "checklist") }
                else { ProgressView().frame(maxWidth: .infinity) }
            } else {
                Table(rows, selection: $selection) {
                    TableColumn("Operation", value: \.operation)
                    TableColumn("Status") { Text($0.status.capitalized).foregroundStyle(stateColor($0.status)) }
                    TableColumn("Progress") { t in
                        ProgressView(value: min(max((t.progress ?? 0) / 100, 0), 1)).frame(width: 90)
                    }
                    TableColumn("Message") { Text($0.message ?? "").lineLimit(2).help($0.message ?? "") }
                    TableColumn("Created", value: \.createdAt)
                }
            }
        }.padding(24)
    }
}

struct AlertsView: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Text("Alerts").font(.largeTitle.weight(.semibold))
                Spacer()
                Text("\(model.unackedAlerts) unacknowledged").foregroundStyle(.secondary)
            }
            PlatformErrorBanner(text: model.platformError)
            if model.alerts.isEmpty {
                if model.platformLoaded { ContentUnavailableView("No alerts", systemImage: "bell.slash") }
                else { ProgressView().frame(maxWidth: .infinity) }
            } else {
                // Newest first; the server returns oldest first.
                List(model.alerts.sorted { $0.timestamp > $1.timestamp }) { a in
                    HStack(alignment: .top, spacing: 10) {
                        Image(systemName: a.severity.lowercased() == "critical" ? "exclamationmark.octagon.fill" : "exclamationmark.triangle.fill")
                            .foregroundStyle(stateColor(a.severity))
                        VStack(alignment: .leading, spacing: 2) {
                            Text(a.ruleName).fontWeight(a.acknowledged ? .regular : .semibold)
                            Text(a.message).font(.callout).foregroundStyle(.secondary)
                        }
                        Spacer()
                        Text(a.timestamp).font(.caption).foregroundStyle(.secondary).monospacedDigit()
                    }.opacity(a.acknowledged ? 0.55 : 1)
                }
            }
        }.padding(24)
    }
}
