import SwiftUI
import MachinaKit

struct VMDetail: View {
    let vm: VmInfo
    @Binding var confirmStop: VmInfo?
    enum Tab: String, CaseIterable, Identifiable { case overview = "Overview", doctor = "Doctor", console = "Console", metrics = "Metrics", disks = "Disks", network = "Network", snapshots = "Snapshots", backups = "Backups", logs = "Logs", settings = "Settings"
        var id: String { rawValue } }
    @State private var tab: Tab = .overview

    var body: some View {
        VStack(spacing: 0) {
            Picker("", selection: $tab) { ForEach(Tab.allCases) { Text($0.rawValue).tag($0) } }
                .pickerStyle(.segmented).labelsHidden().padding([.horizontal, .top], 24)
            Group {
                switch tab {
                case .overview: VMOverview(vm: vm, confirmStop: $confirmStop)
                case .doctor: VMDoctorTab(vm: vm)
                case .console: VMConsoleTab(vm: vm)
                case .metrics: VMMetricsTab(vm: vm)
                case .logs: VMLogsTab(vm: vm)
                case .disks: VMDisksTab(vm: vm)
                case .network: VMNetworkTab(vm: vm)
                case .snapshots: VMSnapshotsTab(vm: vm)
                case .backups: VMBackupsTab(vm: vm)
                case .settings: VMSettingsTab(vm: vm)
                }
            }.id(vm.name + tab.rawValue)
        }
    }
}

/// Shared chrome for a tab: title row with action buttons, optional note, content.
private struct TabFrame<Buttons: View, Content: View>: View {
    let title: String
    var note: String?
    @ViewBuilder var buttons: Buttons
    @ViewBuilder var content: Content
    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack { Text(title).font(.title2.weight(.semibold)); Spacer(); buttons }
            if let note { Text(note).font(.callout).foregroundStyle(.secondary) }
            content
        }.padding(24).frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
    }
}

// MARK: Disks

struct VMDisksTab: View {
    @Environment(AppModel.self) private var model
    let vm: VmInfo
    @State private var details: VmDetails?
    @State private var selection: VmDisk.ID?
    private var selected: VmDisk? { details?.disks?.first { $0.id == selection } }

    var body: some View {
        TabFrame(title: "Disks", note: vm.isRunning ? nil : "Resizing a disk needs the VM to be running.") {
            Button("Attach…") { Task { await attach() } }.disabled(!model.canWrite)
            Button("Resize…") { resize() }.disabled(!model.canWrite || selected == nil || selected?.isCdrom == true || !vm.isRunning)
            if selected?.isCdrom == true {
                Button("Eject") { if let t = selected?.target { Task { await act("Eject \(t)") { try await $0.ejectCdrom(vm, target: t) } } } }
                    .disabled(!model.canWrite)
            }
            Button("Detach") { detach() }.disabled(!model.canWrite || selected == nil || selected?.isCdrom == true)
        } content: {
            Table(details?.disks ?? [], selection: $selection) {
                TableColumn("Target", value: \.target)
                TableColumn("Device") { Text($0.device ?? "–") }
                TableColumn("Source") { Text($0.source ?? "–").textSelection(.enabled).lineLimit(1) }
                TableColumn("Driver") { Text($0.driver ?? "–") }
                TableColumn("Bus") { Text($0.bus ?? "–") }
            }
        }.task(id: vm.name + vm.state) { await load() }
    }

    private func load() async { details = try? await model.api().details(vm) }

    private func act(_ label: String, _ op: @escaping @Sendable (MachinaClient) async throws -> Void) async {
        if await model.perform(label, op) { await load(); await model.refresh() }
    }

    private func attach() async {
        guard let c = try? model.api(), let pools = try? await c.call(.get("/api/v1/storage/pools")).array else { return }
        var options: [String] = []   // "pool / volume → path" so one picker carries the path
        for p in pools { if let name = p["name"]?.text, let vols = try? await c.volumes(pool: name) {
            options += vols.compactMap { v in v.path.map { "\(name) / \(v.name)  →  \($0)" } } } }
        guard !options.isEmpty else { model.error = "No storage volumes found to attach."; return }
        let used = Set(details?.disks?.map(\.target) ?? [])
        let target = ["vdb", "vdc", "vdd", "vde", "vdf"].first { !used.contains($0) } ?? "vdb"
        model.ask("Attach disk", message: "Attach an existing volume to \(vm.name).", submit: "Attach",
                  fields: [.init(id: "vol", label: "Volume", options: options), .init(id: "target", label: "Target", initial: target)]) { v in
            guard let path = v["vol"]?.components(separatedBy: "→").last?.trimmingCharacters(in: .whitespaces), let t = v["target"] else { return }
            await act("Attach disk") { try await $0.attachDisk(vm, target: t, source: path) }
        }
    }

    private func resize() {
        guard let t = selected?.target else { return }
        model.ask("Resize \(t)", message: "Disks can only grow.", submit: "Resize", fields: [.init(id: "gb", label: "New size (GB)")]) { v in
            guard let gb = Int(v["gb"] ?? ""), gb > 0 else { model.error = "Enter a whole number of GB."; return }
            await act("Resize \(t)") { try await $0.resizeDisk(vm, target: t, sizeGb: gb) }
        }
    }

    private func detach() {
        guard let t = selected?.target else { return }
        model.confirmAction("Detach \(t)?", message: "The disk is removed from \(vm.name). The volume itself is kept.", label: "Detach") {
            await act("Detach \(t)") { try await $0.detachDisk(vm, target: t) }
        }
    }
}

// MARK: Network

struct VMNetworkTab: View {
    @Environment(AppModel.self) private var model
    let vm: VmInfo
    @State private var details: VmDetails?
    @State private var selection: VmNic.ID?

    var body: some View {
        TabFrame(title: "Network interfaces") {
            Button("Attach…") { Task { await attach() } }.disabled(!model.canWrite)
            Button("Detach") { detach() }.disabled(!model.canWrite || selection == nil)
        } content: {
            Table(details?.interfaces ?? [], selection: $selection) {
                TableColumn("MAC") { Text($0.macAddress).textSelection(.enabled) }
                TableColumn("Network") { Text($0.source ?? "–") }
                TableColumn("Model") { Text($0.model ?? "–") }
            }
        }.task(id: vm.name + vm.state) { await load() }
    }

    private func load() async { details = try? await model.api().details(vm) }

    private func attach() async {
        guard let nets = try? await model.api().call(.get("/api/v1/networks")).array?.compactMap({ $0["name"]?.text }), !nets.isEmpty else {
            model.error = "No networks found."; return
        }
        model.ask("Attach interface", submit: "Attach", fields: [.init(id: "net", label: "Network", options: nets),
                                                                .init(id: "model", label: "Model", options: ["virtio", "e1000", "rtl8139"])]) { v in
            if await model.perform("Attach interface", { try await $0.attachNic(vm, network: v["net"] ?? "default", model: v["model"] ?? "virtio") }) { await load() }
        }
    }

    private func detach() {
        guard let mac = selection else { return }
        model.confirmAction("Detach \(mac)?", message: "The interface is removed from \(vm.name).", label: "Detach") {
            if await model.perform("Detach interface", { try await $0.detachNic(vm, mac: mac) }) { await load() }
        }
    }
}

// MARK: Snapshots

struct VMSnapshotsTab: View {
    @Environment(AppModel.self) private var model
    let vm: VmInfo
    @State private var snaps: [VmSnapshot] = []
    @State private var loaded = false
    @State private var selection: VmSnapshot.ID?

    var body: some View {
        TabFrame(title: "Snapshots") {
            Button("Take snapshot…") { take() }.disabled(!model.canWrite)
            Button("Revert") { revert() }.disabled(!model.canWrite || selection == nil)
            Button("Delete") { delete() }.disabled(!model.canWrite || selection == nil)
        } content: {
            if loaded && snaps.isEmpty { ContentUnavailableView("No snapshots", systemImage: "camera.on.rectangle") }
            else {
                Table(snaps, selection: $selection) {
                    TableColumn("Name", value: \.name)
                    TableColumn("Created") { s in Text(s.creationTime.map { Date(timeIntervalSince1970: $0).formatted(date: .abbreviated, time: .shortened) } ?? "–") }
                    TableColumn("State") { Text($0.state ?? "–") }
                    TableColumn("Description") { Text($0.description ?? "") }
                    TableColumn("Current") { Text($0.isCurrent == true ? "yes" : "") }
                }
            }
        }.task(id: vm.name) { await load() }
    }

    private func load() async { snaps = (try? await model.api().snapshots(vm)) ?? []; loaded = true }

    private func take() {
        let stamp = Date().formatted(.verbatim("\(year: .defaultDigits)\(month: .twoDigits)\(day: .twoDigits)-\(hour: .twoDigits(clock: .twentyFourHour, hourCycle: .zeroBased))\(minute: .twoDigits)", locale: .current, timeZone: .current, calendar: .current))
        model.ask("Take snapshot", message: "Snapshot of \(vm.name) (\(vm.state)).", submit: "Take snapshot",
                  fields: [.init(id: "name", label: "Name", initial: "snap-\(stamp)"), .init(id: "desc", label: "Description")]) { v in
            guard let n = v["name"], !n.isEmpty else { return }
            if await model.perform("Snapshot", { try await $0.createSnapshot(vm, name: n, description: v["desc"] ?? "") }) { await load(); model.notice = "Snapshot \(n) created" }
        }
    }

    private func revert() {
        guard let s = selection else { return }
        model.confirmAction("Revert to \(s)?", message: "\(vm.name) goes back to this snapshot. Changes made since then are lost.", label: "Revert") {
            if await model.perform("Revert", { try await $0.revertSnapshot(vm, s) }) { await load(); await model.refresh() }
        }
    }

    private func delete() {
        guard let s = selection else { return }
        model.confirmAction("Delete snapshot \(s)?", message: "This cannot be undone.", label: "Delete") {
            if await model.perform("Delete snapshot", { try await $0.deleteSnapshot(vm, s) }) { await load() }
        }
    }
}

// MARK: Backups

struct VMBackupsTab: View {
    @Environment(AppModel.self) private var model
    let vm: VmInfo
    @State private var backups: [BackupInfo] = []
    @State private var loaded = false
    @State private var selection: BackupInfo.ID?

    var body: some View {
        TabFrame(title: "Backups") {
            Button("Back up now…") { backUp() }.disabled(!model.canWrite)
            Button("Verify") { verify() }.disabled(selection == nil)
            Button("Restore") { restore() }.disabled(!model.canWrite || selection == nil)
            Button("Delete") { delete() }.disabled(!model.canWrite || selection == nil)
        } content: {
            if loaded && backups.isEmpty { ContentUnavailableView("No backups for this VM", systemImage: "externaldrive.badge.timemachine") }
            else {
                Table(backups, selection: $selection) {
                    TableColumn("ID", value: \.id)
                    TableColumn("Time") { Text($0.timestamp ?? "–") }
                    TableColumn("Size") { Text($0.size ?? "–") }
                    TableColumn("Status") { Text($0.status ?? "–") }
                    TableColumn("Disks") { Text($0.withDisks == true ? "yes" : "no") }
                }
            }
        }.task(id: vm.name) { await load() }
    }

    private func load() async {
        backups = ((try? await model.api().backups()) ?? []).filter { ($0.vmFilter ?? "") == vm.name }
        loaded = true
    }

    private func backUp() {
        model.ask("Back up \(vm.name)", message: "Including disks can take a while and a lot of space.", submit: "Back up",
                  fields: [.init(id: "disks", label: "Include disks", options: ["yes", "no"])]) { v in
            if await model.perform("Backup", { try await $0.backUp(vm, withDisks: v["disks"] == "yes") }) { await load(); model.notice = "Backup started" }
        }
    }

    private func verify() {
        guard let id = selection else { return }
        Task {
            do {
                let r = try await model.api().verifyBackup(id)
                model.notice = r["verified"]?.text == "yes" ? "Backup \(id) verified (\(r["files_ok"]?.text ?? "?") files)" : "Backup \(id): verification reported problems"
            } catch { model.error = "Verify: \(error.localizedDescription)" }
        }
    }

    private func restore() {
        guard let id = selection else { return }
        model.confirmAction("Restore \(id)?", message: "This overwrites \(vm.name) with the backup.", label: "Restore") {
            if await model.perform("Restore", { try await $0.restoreBackup(id) }) { model.notice = "Restore started"; await model.refresh() }
        }
    }

    private func delete() {
        guard let id = selection else { return }
        model.confirmAction("Delete backup \(id)?", message: "This cannot be undone.", label: "Delete") {
            if await model.perform("Delete backup", { try await $0.deleteBackup(id) }) { await load() }
        }
    }
}

// MARK: Settings

struct VMSettingsTab: View {
    @Environment(AppModel.self) private var model
    let vm: VmInfo
    @State private var details: VmDetails?
    @State private var autostart = false

    var body: some View {
        TabFrame(title: "Settings") { EmptyView() } content: {
            Form {
                Toggle("Start automatically with the host", isOn: $autostart)
                    .disabled(!model.canWrite)
                    .onChange(of: autostart) { old, new in
                        guard details != nil, new != (details?.autostart ?? false) else { return }
                        Task { if await model.perform("Autostart", { try await $0.setAutostart(vm, new) }) { await load() } else { autostart = old } }
                    }
                LabeledContent("vCPUs") { HStack { Text("\(details?.vcpus ?? vm.vcpus)"); Button("Change…") { vcpus() }.disabled(!model.canWrite) } }
                LabeledContent("Memory") { HStack { Text("\(details?.memoryMb ?? vm.memoryMb) MiB"); Button("Change…") { memory() }.disabled(!model.canWrite) } }
                Section("Manage") {
                    Button("Clone…") { clone() }.disabled(!model.canWrite)
                    Button("Rename…") { rename() }.disabled(!model.canWrite)
                    Button("Delete VM…", role: .destructive) { delete() }.disabled(!model.canWrite)
                }
            }.formStyle(.grouped)
        }.task(id: vm.name) { await load() }
    }

    private func load() async { details = try? await model.api().details(vm); autostart = details?.autostart ?? false }

    private func number(_ s: String?) -> Int? { Int(s ?? "").flatMap { $0 > 0 ? $0 : nil } }

    private func vcpus() {
        model.ask("vCPUs for \(vm.name)", message: "The server may require the VM to be stopped.", submit: "Apply", fields: [.init(id: "n", label: "vCPUs", initial: "\(details?.vcpus ?? vm.vcpus)")]) { v in
            guard let n = number(v["n"]) else { model.error = "Enter a whole number."; return }
            if await model.perform("vCPUs", { try await $0.setVcpus(vm, n) }) { await load(); await model.refresh() }
        }
    }

    private func memory() {
        model.ask("Memory for \(vm.name)", submit: "Apply", fields: [.init(id: "mb", label: "Memory (MiB)", initial: "\(details?.memoryMb ?? vm.memoryMb)")]) { v in
            guard let n = number(v["mb"]) else { model.error = "Enter a whole number."; return }
            if await model.perform("Memory", { try await $0.setMemory(vm, mb: n) }) { await load(); await model.refresh() }
        }
    }

    private func clone() {
        model.ask("Clone \(vm.name)", message: "Linked clones need the source VM to be stopped; full clones copy the disks.", submit: "Clone",
                  fields: [.init(id: "name", label: "New name", initial: vm.name + "-clone"), .init(id: "mode", label: "Mode", options: ["linked", "full", "xml"])]) { v in
            guard let n = v["name"], !n.isEmpty else { return }
            if await model.perform("Clone", { try await $0.clone(vm, as: n, mode: CloneMode(rawValue: v["mode"] ?? "") ?? .linked) }) {
                await model.refresh(); model.notice = "Cloned to \(n)"
            }
        }
    }

    private func rename() {
        model.ask("Rename \(vm.name)", message: "The VM must be stopped.", submit: "Rename", fields: [.init(id: "name", label: "New name", initial: vm.name)]) { v in
            guard let n = v["name"], !n.isEmpty, n != vm.name else { return }
            if await model.perform("Rename", { try await $0.rename(vm, to: n) }) { await model.refresh(); model.selection = .vm(n) }
        }
    }

    private func delete() {
        model.ask("Delete \(vm.name)?", message: "This removes the VM. Type its name to confirm.", submit: "Delete",
                  fields: [.init(id: "confirm", label: "VM name")]) { v in
            guard v["confirm"] == vm.name else { model.error = "Name did not match; nothing was deleted."; return }
            if await model.perform("Delete", { try await $0.deleteVM(vm) }) { await model.refresh(); model.selection = .fleet; model.notice = "Deleted \(vm.name)" }
        }
    }
}

// MARK: New VM

extension AppModel {
    func presentCreateVM() async {
        let nets = (try? await api().call(.get("/api/v1/networks")).array?.compactMap { $0["name"]?.text }) ?? ["default"]
        ask("New VM", message: "The server starts the VM right after creating it.", submit: "Create",
            fields: [.init(id: "name", label: "Name"), .init(id: "vcpus", label: "vCPUs", initial: "2"),
                     .init(id: "mem", label: "Memory (MiB)", initial: "2048"), .init(id: "disk", label: "Disk (GB)", initial: "20"),
                     .init(id: "net", label: "Network", options: nets.isEmpty ? ["default"] : nets),
                     .init(id: "iso", label: "Install ISO path (optional)")]) { [self] v in
            guard let name = v["name"], !name.isEmpty, let c = Int(v["vcpus"] ?? ""), let m = Int(v["mem"] ?? ""), let d = Int(v["disk"] ?? "") else {
                error = "Fill in the name and numeric sizes."; return
            }
            let req = CreateVMRequest(name: name, vcpus: c, memoryMb: m, diskGb: d, iso: v["iso"], network: v["net"])
            if await perform("Create VM", { try await $0.createVM(req) }) { await refresh(); selection = .vm(name); notice = "Created \(name)" }
        }
    }
}
