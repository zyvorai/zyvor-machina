import SwiftUI
import Charts
import MachinaKit

/// A titled multi-series line chart.
struct SeriesChart: View {
    let title: String
    let series: [SeriesPoint]
    var format: (Double) -> String = { String(format: "%.0f", $0) }
    var fixedMax: Double?

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                Text(title).font(.headline)
                Spacer()
                if let last = Dictionary(grouping: series, by: \.series).mapValues({ $0.last?.value ?? 0 }).sorted(by: { $0.key < $1.key }) as [(key: String, value: Double)]?, !last.isEmpty {
                    Text(last.map { "\($0.key) \(format($0.value))" }.joined(separator: "  ·  ")).font(.caption).foregroundStyle(.secondary).monospacedDigit()
                }
            }
            if series.isEmpty {
                Text("No samples yet").font(.callout).foregroundStyle(.secondary).frame(maxWidth: .infinity, minHeight: 120)
            } else {
                Chart(series) { p in
                    LineMark(x: .value("Time", p.date), y: .value(title, p.value)).foregroundStyle(by: .value("Series", p.series))
                        .interpolationMethod(.monotone)
                }
                .chartYScale(domain: 0...(fixedMax ?? max(series.map(\.value).max() ?? 1, 1) * 1.1))
                .chartYAxis { AxisMarks { v in AxisGridLine(); AxisValueLabel { if let d = v.as(Double.self) { Text(format(d)) } } } }
                .frame(height: 150)
            }
        }
        .padding(14).background(.quaternary.opacity(0.4), in: .rect(cornerRadius: 10))
    }
}

func bytesPerSecond(_ v: Double) -> String { ByteCountFormatter.string(fromByteCount: Int64(v), countStyle: .binary) + "/s" }

struct HostHistoryChart: View {
    @Environment(AppModel.self) private var model
    var body: some View {
        SeriesChart(title: "Host utilisation (%)", series: MetricsMath.hostSeries(model.history), format: { String(format: "%.0f%%", $0) }, fixedMax: 100)
    }
}

struct VMMetricsTab: View {
    @Environment(AppModel.self) private var model
    let vm: VmInfo
    @State private var points: [MetricsPoint] = []
    @State private var window = 120
    @State private var loadError: String?

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 14) {
                HStack {
                    Text("Metrics").font(.title2.weight(.semibold))
                    Spacer()
                    Picker("Samples", selection: $window) { Text("30").tag(30); Text("60").tag(60); Text("120").tag(120); Text("240").tag(240) }
                        .pickerStyle(.segmented).frame(width: 220).labelsHidden()
                }
                PlatformErrorBanner(text: loadError)
                if !vm.isRunning { Text("This VM is not running; charts show its last recorded samples.").font(.callout).foregroundStyle(.secondary) }
                SeriesChart(title: "CPU (% of allotted)", series: MetricsMath.vmCPU(points, vm: vm.name), format: { String(format: "%.0f%%", $0) }, fixedMax: 100)
                SeriesChart(title: "Memory used (MiB)", series: MetricsMath.vmMemory(points, vm: vm.name), format: { String(format: "%.0f", $0) })
                SeriesChart(title: "Disk throughput", series: MetricsMath.vmRates(points, vm: vm.name, [.diskRead, .diskWrite]), format: bytesPerSecond)
                SeriesChart(title: "Network throughput", series: MetricsMath.vmRates(points, vm: vm.name, [.netRx, .netTx]), format: bytesPerSecond)
            }.padding(24)
        }
        .task(id: "\(vm.name)-\(window)") {
            while !Task.isCancelled {
                do { points = try await model.api().metricsHistory(limit: window); loadError = nil } catch { loadError = error.localizedDescription }
                try? await Task.sleep(nanoseconds: 10_000_000_000)
            }
        }
    }
}

struct VMLogsTab: View {
    @Environment(AppModel.self) private var model
    let vm: VmInfo
    @State private var text = ""
    @State private var filter = ""
    @State private var loadError: String?

    private var shown: String {
        // Last ~300 KB; filtered line by line.
        let tail = text.count > 300_000 ? String(text.suffix(300_000)) : text
        return filter.isEmpty ? tail : tail.split(separator: "\n", omittingEmptySubsequences: false).filter { $0.localizedCaseInsensitiveContains(filter) }.joined(separator: "\n")
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack {
                Text("Log").font(.title2.weight(.semibold)); Spacer()
                TextField("Filter", text: $filter).textFieldStyle(.roundedBorder).frame(width: 220)
                Button { Task { await load() } } label: { Image(systemName: "arrow.clockwise") }
            }
            PlatformErrorBanner(text: loadError)
            ScrollViewReader { proxy in
                ScrollView {
                    Text(shown.isEmpty ? "No log output." : shown).font(.system(.caption, design: .monospaced)).textSelection(.enabled)
                        .frame(maxWidth: .infinity, alignment: .leading)
                    Color.clear.frame(height: 1).id("end")
                }
                .padding(10).background(.quaternary.opacity(0.4), in: .rect(cornerRadius: 8))
                .onChange(of: shown) { proxy.scrollTo("end") }
            }
        }
        .padding(24).task(id: vm.name) { await load() }
    }

    private func load() async {
        do { text = try await model.api().vmLog(vm); loadError = nil } catch { loadError = error.localizedDescription }
    }
}
