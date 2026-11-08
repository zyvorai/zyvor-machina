import SwiftUI
import MachinaKit

struct StateBadge: View {
    let state: String
    var color: Color {
        switch state.lowercased() {
        case "running": .green
        case "paused": .orange
        case "crashed", "error": .red
        default: .gray
        }
    }
    var body: some View {
        Label(state.capitalized, systemImage: "circle.fill")
            .labelStyle(.titleAndIcon).foregroundStyle(color).font(.callout)
    }
}

struct MainWindow: View {
    @Environment(AppModel.self) private var model
    @State private var search = ""
    @State private var confirmStop: VmInfo?
    @State private var dropTargeted = false
    @Environment(\.openWindow) private var openWindow
    // Long sections start collapsed; the Cmd-K palette reaches every page regardless.
    @State private var collapsed: Set<String> = ["Containers", "Kubernetes", "Control plane", "Security", "Fleet security", "Bare metal", "Insight", "Storage & network", "Fleet Cloud"]

    private var filtered: [VmInfo] {
        search.isEmpty ? model.vms : model.vms.filter { $0.name.localizedCaseInsensitiveContains(search) }
    }
    private var selectedVM: VmInfo? {
        if case .vm(let n) = model.selection { return model.vms.first { $0.name == n } }
        return nil
    }
    private var wantsPlatform: Bool {
        switch model.selection { case .fleet, .tasks, .alerts: true; default: false }
    }

    var body: some View {
        @Bindable var model = model
        NavigationSplitView {
            List(selection: $model.selection) {
                Section("Overview") {
                    Label("Fleet", systemImage: "server.rack").tag(SidebarItem.fleet)
                    Label("Tasks", systemImage: "checklist").tag(SidebarItem.tasks)
                    Label("Alerts", systemImage: "bell").badge(model.unackedAlerts).tag(SidebarItem.alerts)
                }
                ForEach(FeatureRegistry.groups, id: \.name) { group in
                    Section(group.name, isExpanded: Binding(get: { !collapsed.contains(group.name) },
                                                           set: { if $0 { collapsed.remove(group.name) } else { collapsed.insert(group.name) } })) {
                        ForEach(group.specs) { spec in
                            Label(spec.title, systemImage: spec.icon).badge(spec.id == "approvals" ? model.pendingApprovals : 0).tag(SidebarItem.page(spec.id))
                        }
                    }
                }
                Section("VMs") {
                    ForEach(filtered) { vm in
                        HStack {
                            Text(vm.name)
                            Spacer()
                            Image(systemName: "circle.fill").font(.caption2)
                                .foregroundStyle(vm.isRunning ? .green : .gray)
                        }.tag(SidebarItem.vm(vm.name))
                        .contextMenu { Button("Open in New Window") { openWindow(id: "vm-window", value: vm.name) } }
                    }
                }
            }
            .searchable(text: $search, placement: .sidebar, prompt: "Search VMs")
            .navigationSplitViewColumnWidth(min: 200, ideal: 240)
            .safeAreaInset(edge: .bottom) {
                HStack {
                    Circle().fill(model.live ? .green : .orange).frame(width: 8, height: 8)
                    Text(model.live ? "Live" : "Reconnecting").font(.caption).foregroundStyle(.secondary)
                    Spacer()
                    if let r = model.role { Text(r).font(.caption).foregroundStyle(.secondary) }
                    Text("\(model.runningCount)/\(model.vms.count) running").font(.caption).foregroundStyle(.secondary)
                }.padding(10)
            }
        } detail: {
            switch model.selection {
            case .fleet: FleetView()
            case .tasks: TasksView()
            case .alerts: AlertsView()
            case .page(let id):
                if let spec = FeatureRegistry.spec(id) { ResourceListView(spec: spec).id(id) }
                else { ContentUnavailableView("Unknown page", systemImage: "questionmark.folder") }
            case .vm: if let vm = selectedVM { VMDetail(vm: vm, confirmStop: $confirmStop) }
                      else { ContentUnavailableView("VM not found", systemImage: "server.rack") }
            case nil: ContentUnavailableView("Select an item", systemImage: "sidebar.left")
            }
        }
        // Poll the platform endpoints every 10 s while a fleet/tasks/alerts page is open,
        // and once at start so the Alerts badge is filled in.
        .task(id: wantsPlatform) {
            await model.refreshPlatform()
            guard wantsPlatform else { return }
            while !Task.isCancelled {
                try? await Task.sleep(nanoseconds: 10_000_000_000)
                if Task.isCancelled { break }
                await model.refreshPlatform()
            }
        }
        .onAppear { if model.selection == nil { model.selection = .fleet } }
        .task {
            while !Task.isCancelled {
                await model.refreshApprovals()
                try? await Task.sleep(nanoseconds: 30_000_000_000)
            }
        }
        .dropDestination(for: URL.self) { urls, _ in model.handleDrop(urls) } isTargeted: { dropTargeted = $0 }
        .overlay {
            if dropTargeted {
                RoundedRectangle(cornerRadius: 14).strokeBorder(Color.accentColor, style: StrokeStyle(lineWidth: 3, dash: [10, 6])).padding(6)
                    .overlay { Label("Drop .iso files to upload", systemImage: "opticaldisc").font(.title2.weight(.semibold)).padding(16).background(.regularMaterial, in: .rect(cornerRadius: 12)) }
                    .allowsHitTesting(false)
            }
        }
        .sheet(isPresented: $model.showPalette) { CommandPalette().environment(model) }
        .prompt($model.prompt)
        .confirm($model.pendingAction)
        .background { Button("") { model.showPalette = true }.keyboardShortcut("k", modifiers: .command).opacity(0) }
        .toolbar {
            ToolbarItem { Button { Task { await model.presentCreateVM() } } label: { Image(systemName: "plus") }.help("New VM").disabled(!model.canWrite) }
            ToolbarItem { Button { Task { await model.refresh(); await model.refreshPlatform() } } label: { Image(systemName: "arrow.clockwise") } }
            ToolbarItem {
                Menu {
                    ForEach(model.servers) { p in
                        Button("\(p.name)\(p.username.map { " (\($0))" } ?? "")\(model.hasSavedPassword(p) ? "" : " — password needed")") { Task { await model.switchTo(p) } }
                    }
                    Divider()
                    Button("Sign Out") { Task { await model.signOut() } }
                } label: { Label("Server", systemImage: "externaldrive.connected.to.line.below") }
            }
        }
        .confirmationDialog("Force stop \(confirmStop?.name ?? "")?", isPresented: .constant(confirmStop != nil),
                            titleVisibility: .visible) {
            Button("Force Stop", role: .destructive) {
                if let vm = confirmStop { Task { await model.power(.stop, vm) } }
                confirmStop = nil
            }
            Button("Cancel", role: .cancel) { confirmStop = nil }
        } message: { Text("This is the equivalent of pulling the power cord.") }
        .overlay(alignment: .bottom) {
            VStack(spacing: 6) {
                if let n = model.notice {
                    Text(n).padding(8).background(.green.opacity(0.9), in: .capsule).foregroundStyle(.white)
                        .onTapGesture { model.notice = nil }
                }
                if let e = model.error {
                    Text(e).padding(8).background(.red.opacity(0.9), in: .capsule).foregroundStyle(.white)
                        .lineLimit(4).onTapGesture { model.error = nil }
                }
            }.padding()
        }
    }
}

struct VMOverview: View {
    @Environment(AppModel.self) private var model
    let vm: VmInfo
    @Binding var confirmStop: VmInfo?

    var body: some View {
        VStack(alignment: .leading, spacing: 20) {
            HStack { Text(vm.name).font(.largeTitle.weight(.semibold)); StateBadge(state: vm.state) }
            Grid(alignment: .leading, horizontalSpacing: 24, verticalSpacing: 8) {
                GridRow { Text("vCPUs").foregroundStyle(.secondary); Text("\(vm.vcpus)") }
                GridRow { Text("Memory").foregroundStyle(.secondary); Text("\(vm.memoryMb) MiB") }
                if let ip = vm.guestIp { GridRow { Text("Guest IP").foregroundStyle(.secondary); Text(ip).textSelection(.enabled) } }
                if let b = vm.backend { GridRow { Text("Backend").foregroundStyle(.secondary); Text(b) } }
                if let c = vm.libvirtConnection { GridRow { Text("Connection").foregroundStyle(.secondary); Text(c) } }
            }
            HStack {
                if vm.isRunning {
                    action("Shut Down", "power", .shutdown)
                    action("Reboot", "arrow.clockwise", .reboot)
                    action("Pause", "pause.fill", .pause)
                    Button(role: .destructive) { confirmStop = vm } label: { Label("Force Stop", systemImage: "stop.fill") }
                } else if vm.state.lowercased() == "paused" {
                    action("Resume", "play.fill", .resume)
                } else {
                    action("Start", "play.fill", .start)
                }
            }
            Spacer()
        }
        .padding(28).frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
    }

    private func action(_ title: String, _ icon: String, _ a: PowerAction) -> some View {
        Button { Task { await model.power(a, vm) } } label: { Label(title, systemImage: icon) }
    }
}

struct MenuBarContent: View {
    @Environment(AppModel.self) private var model
    @Environment(\.openWindow) private var openWindow

    var body: some View {
        if model.phase != .ready {
            Text("Not connected")
        } else {
            Text("\(model.runningCount) of \(model.vms.count) VMs running")
            Divider()
            ForEach(model.vms.prefix(15)) { vm in
                Menu("\(vm.isRunning ? "●" : "○") \(vm.name)") {
                    if vm.isRunning {
                        Button("Shut Down") { Task { await model.power(.shutdown, vm) } }
                        Button("Reboot") { Task { await model.power(.reboot, vm) } }
                    } else {
                        Button("Start") { Task { await model.power(.start, vm) } }
                    }
                }
            }
        }
        Divider()
        Button("Open Machina") { openWindow(id: "main"); NSApp.activate(ignoringOtherApps: true) }
        Button("Quit") { NSApp.terminate(nil) }
    }
}
