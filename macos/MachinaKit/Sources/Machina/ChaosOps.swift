import SwiftUI
import MachinaKit

/// Create / edit sheet for a chaos experiment. Admin only. Nothing is targeted until the user ticks VMs, and the
/// save confirmation lists the blast radius (targets, steps, longest run).
struct ChaosEditor: View {
    @Environment(AppModel.self) private var model
    let editingID: String?
    let onDone: (Bool) -> Void

    @State private var draft: ChaosDraft
    @State private var vms: [(id: String, name: String)] = []
    @State private var vmFilter = ""
    @State private var kind = "latency"
    @State private var a = "100"
    @State private var b = "0"
    @State private var c = "30"
    @State private var cidrs = ""
    @State private var probeText = ""
    @State private var problem: String?
    @State private var busy = false
    @State private var pending: Endpoint?

    init(row: ResourceRow?, onDone: @escaping (Bool) -> Void) {
        editingID = row?.id
        _draft = State(initialValue: row.map { ChaosDraft(row: $0.values) } ?? ChaosDraft())
        self.onDone = onDone
    }

    private var names: [String: String] { Dictionary(vms.map { ($0.id, $0.name) }, uniquingKeysWith: { x, _ in x }) }
    private var shownVMs: [(id: String, name: String)] {
        vmFilter.isEmpty ? vms : vms.filter { $0.name.localizedCaseInsensitiveContains(vmFilter) }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(editingID == nil ? "New chaos experiment" : "Edit chaos experiment").font(.title2.weight(.semibold))
            Text("Experiments inject real faults when run. Nothing is selected by default; tag a VM chaos=protected to make the controller refuse it.")
                .font(.callout).foregroundStyle(.secondary)
            ScrollView {
                VStack(alignment: .leading, spacing: 14) {
                    TextField("Name", text: $draft.name).textFieldStyle(.roundedBorder)
                    TextField("Description", text: $draft.description).textFieldStyle(.roundedBorder)
                    GroupBox("Target VMs (\(draft.targets.count) selected)") {
                        VStack(alignment: .leading) {
                            TextField("Filter", text: $vmFilter).textFieldStyle(.roundedBorder)
                            List(shownVMs, id: \.id) { vm in
                                Toggle(vm.name, isOn: Binding(get: { draft.targets.contains(vm.id) }, set: { on in
                                    if on { draft.targets.append(vm.id) } else { draft.targets.removeAll { $0 == vm.id } }
                                }))
                            }.frame(height: 130)
                            let unknown = draft.targets.filter { names[$0] == nil }
                            if !unknown.isEmpty { Text("\(unknown.count) selected target(s) are not in the VM list.").font(.caption).foregroundStyle(.orange) }
                        }
                    }
                    GroupBox("Steps (\(draft.steps.count))") {
                        VStack(alignment: .leading, spacing: 6) {
                            ForEach(Array(draft.steps.enumerated()), id: \.offset) { i, s in
                                HStack {
                                    Text("\(i + 1). \(s.label)")
                                    Spacer()
                                    Button { draft.steps.remove(at: i) } label: { Image(systemName: "minus.circle") }.buttonStyle(.borderless).accessibilityLabel("Remove step \(i + 1)")
                                }
                            }
                            stepAdder
                        }
                    }
                    GroupBox("Probes (\(draft.probes.count))") {
                        VStack(alignment: .leading, spacing: 6) {
                            ForEach(Array(draft.probes.enumerated()), id: \.offset) { i, p in
                                HStack {
                                    Text("\(p["kind"]?.text ?? "?") \(p["target"]?.text ?? p["url"]?.text ?? p["vm"]?.text ?? "")").lineLimit(1)
                                    Spacer()
                                    Button { draft.probes.remove(at: i) } label: { Image(systemName: "minus.circle") }.buttonStyle(.borderless).accessibilityLabel("Remove probe \(i + 1)")
                                }
                            }
                            HStack {
                                TextField("host:port or https://url", text: $probeText).textFieldStyle(.roundedBorder)
                                Button("Add probe") {
                                    let t = probeText.trimmingCharacters(in: .whitespaces)
                                    guard !t.isEmpty else { return }
                                    draft.probes.append(t.hasPrefix("http") ? ChaosDraft.httpProbe(t) : ChaosDraft.tcpProbe(t)); probeText = ""
                                }
                            }
                        }
                    }
                    GroupBox("Safety and timing") {
                        Grid(alignment: .leading, horizontalSpacing: 12, verticalSpacing: 6) {
                            GridRow { Text("Abort below success %"); TextField("", value: $draft.minSuccessPct, format: .number).frame(width: 70) }
                            GridRow { Text("Abort window (s)"); TextField("", value: $draft.windowSecs, format: .number).frame(width: 70) }
                            GridRow { Text("Baseline (s)"); TextField("", value: $draft.baselineSecs, format: .number).frame(width: 70) }
                            GridRow { Text("Recovery (s)"); TextField("", value: $draft.recoverySecs, format: .number).frame(width: 70) }
                        }
                    }
                }.padding(.trailing, 8)
            }
            if let p = problem { Text(p).foregroundStyle(.red).font(.callout) }
            HStack {
                Text("Longest run: \(draft.totalSecs)s").foregroundStyle(.secondary)
                Spacer()
                Button("Cancel") { onDone(false) }.keyboardShortcut(.cancelAction)
                Button(editingID == nil ? "Create…" : "Save…") { review() }
                    .keyboardShortcut(.defaultAction).disabled(busy || !model.isAdmin)
                    .help(model.isAdmin ? "" : "Authoring chaos experiments needs the admin role")
            }
        }
        .padding(20).frame(width: 620, height: 720)
        .task { await loadVMs() }
        .alert("\(editingID == nil ? "Create" : "Save") experiment \(draft.name)?", isPresented: Binding(get: { pending != nil }, set: { if !$0 { pending = nil } })) {
            Button(editingID == nil ? "Create" : "Save") { if let e = pending { send(e) } }
            Button("Cancel", role: .cancel) { pending = nil }
        } message: { Text(draft.blastRadius(vmNames: names) + "\n\nSaving does not run it.") }
    }

    private var stepAdder: some View {
        HStack {
            Picker("Add step", selection: $kind) {
                Text("Latency").tag("latency"); Text("Packet loss").tag("loss"); Text("Partition").tag("partition")
                Text("Disk IOPS").tag("disk"); Text("Kill VM").tag("kill"); Text("Host failure (simulation)").tag("host_failure")
            }.frame(width: 230)
            switch kind {
            case "latency": field("ms", $a); field("jitter", $b); field("secs", $c)
            case "loss": field("%", $a); field("secs", $c)
            case "partition": TextField("CIDRs, comma separated", text: $cidrs).textFieldStyle(.roundedBorder); field("secs", $c)
            case "disk": field("read IOPS", $a); field("write IOPS", $b); field("secs", $c)
            case "kill": field("recover s", $c)
            default: field("host id", $a, width: 150)
            }
            Button("Add") { addStep() }
        }
    }
    private func field(_ label: String, _ v: Binding<String>, width: CGFloat = 56) -> some View {
        TextField(label, text: v).textFieldStyle(.roundedBorder).frame(width: width).help(label)
    }

    private func addStep() {
        let i = { (s: String) in Int(s.trimmingCharacters(in: .whitespaces)) ?? 0 }
        switch kind {
        case "latency": draft.steps.append(.latency(delayMs: i(a), jitterMs: i(b), secs: i(c)))
        case "loss": draft.steps.append(.loss(pct: Double(a.trimmingCharacters(in: .whitespaces)) ?? 0, secs: i(c)))
        case "partition":
            draft.steps.append(.partition(cidrs: cidrs.split(whereSeparator: { ", ".contains($0) }).map(String.init), peers: [], secs: i(c)))
        case "disk": draft.steps.append(.disk(readIops: i(a), writeIops: i(b), secs: i(c)))
        case "kill": draft.steps.append(.kill(recoverSecs: i(c)))
        default: draft.steps.append(.hostFailure(hostID: a.trimmingCharacters(in: .whitespaces)))
        }
    }

    private func loadVMs() async {
        let json = (try? await model.call(.controllerGet("/api/v1/vms"))) ?? .null
        let rows = json.array ?? json["items"]?.array ?? []
        vms = rows.compactMap { r in
            guard let id = r["id"]?.text, let n = r["name"]?.text, !id.isEmpty else { return nil }
            return (id, n)
        }.sorted { $0.name.localizedCaseInsensitiveCompare($1.name) == .orderedAscending }
    }

    private func review() {
        problem = nil
        let endpoint: Endpoint
        do { endpoint = try editingID.map { try ChaosAPI.update(id: $0, draft) } ?? ChaosAPI.create(draft) }
        catch { problem = error.localizedDescription; return }
        pending = endpoint
    }

    private func send(_ e: Endpoint) {
        pending = nil; busy = true
        Task {
            defer { busy = false }
            do { _ = try await model.call(e); onDone(true) }
            catch { problem = error.localizedDescription }
        }
    }
}

struct ChaosSheet: Identifiable { let id = UUID(); let row: ResourceRow? }
