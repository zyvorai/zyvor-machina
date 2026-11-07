import SwiftUI
import AppKit
import MachinaKit

/// Cmd-K: fuzzy search over pages, VMs, saved filters and actions; recents first when the query is empty.
struct CommandPalette: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    @State private var query = ""
    @State private var highlighted = 0

    struct Entry: Identifiable {
        var id: String
        var title: String
        var detail: String
        var icon: String
        var run: @MainActor (AppModel) -> Void
    }

    private func nav(_ id: String, _ title: String, _ detail: String, _ icon: String, _ item: SidebarItem) -> Entry {
        Entry(id: id, title: title, detail: detail, icon: icon) { $0.selection = item }
    }

    private var all: [Entry] {
        var e: [Entry] = [nav("fleet", "Fleet", "Overview", "server.rack", .fleet),
                          nav("tasks", "Tasks", "Overview", "checklist", .tasks),
                          nav("alerts", "Alerts", "Overview", "bell", .alerts)]
        e += FeatureRegistry.all.map { nav("p-" + $0.id, $0.title, $0.group, $0.icon, .page($0.id)) }
        e += model.vms.map { nav("vm-" + $0.name, $0.name, "VM · \($0.state)" + (model.pins.contains($0.name) ? " · pinned" : ""), "desktopcomputer", .vm($0.name)) }
        e += actions
        return e
    }

    private var actions: [Entry] {
        var a: [Entry] = [
            Entry(id: "a-new", title: "New VM…", detail: "Action", icon: "plus") { m in Task { await m.presentCreateVM() } },
            Entry(id: "a-bulk", title: "Bulk actions on VMs…", detail: "Action", icon: "checklist.checked") { $0.showBulk = true },
            Entry(id: "a-refresh", title: "Refresh", detail: "Action", icon: "arrow.clockwise") { m in Task { await m.refresh(); await m.refreshPlatform() } },
            Entry(id: "a-keys", title: "Keyboard shortcuts", detail: "Help", icon: "keyboard") { $0.showShortcuts = true },
            Entry(id: "a-savefilter", title: "Save current VM filter…", detail: "Action", icon: "line.3.horizontal.decrease.circle") { $0.saveCurrentFilter() },
        ]
        if let p = model.page {
            a.append(Entry(id: "a-csv", title: "Export \(p.spec.title) as CSV…", detail: "Action", icon: "square.and.arrow.up") { m in PageExporter.export(m, csv: true) })
            a.append(Entry(id: "a-curl", title: "Copy \(p.spec.title) request as curl", detail: "Action", icon: "terminal") { m in m.copyCurl(p.spec.endpoint) })
        }
        if case .vm(let n) = model.selection, let vm = model.vms.first(where: { $0.name == n }) {
            a.append(Entry(id: "a-pin", title: (model.pins.contains(n) ? "Unpin " : "Pin ") + n, detail: "Action", icon: "pin") { $0.togglePin(n) })
            for act in BulkAction.allCases where act.applies(to: vm) && act != .snapshot {
                a.append(Entry(id: "a-\(act.rawValue)-\(n)", title: "\(act.title) \(n)", detail: "VM action", icon: "bolt") { $0.bulk(act, on: [vm]) })
            }
        }
        for f in model.savedFilters {
            a.append(Entry(id: "f-" + f.name, title: "Filter VMs: \(f.name)", detail: f.query, icon: "line.3.horizontal.decrease.circle") { $0.vmQuery = f.query })
        }
        a.append(Entry(id: "a-signout", title: "Sign Out", detail: "Action", icon: "rectangle.portrait.and.arrow.right") { m in Task { await m.signOut() } })
        return a
    }

    private var entries: [Entry] {
        if query.trimmingCharacters(in: .whitespaces).isEmpty {
            let recent = Recents.promoting(all, recents: model.recents) { $0.id }
            return Array(recent.prefix(40))
        }
        return Array(Fuzzy.rank(all, query: query) { [$0.title, $0.detail] }.prefix(40))
    }

    var body: some View {
        VStack(spacing: 0) {
            TextField("Go to a page or VM, or run an action…", text: $query).textFieldStyle(.plain).font(.title3).padding(14)
                .onSubmit { if entries.indices.contains(highlighted) { run(entries[highlighted]) } }
                .onKeyPress(.downArrow) { highlighted = min(highlighted + 1, max(entries.count - 1, 0)); return .handled }
                .onKeyPress(.upArrow) { highlighted = max(highlighted - 1, 0); return .handled }
                .onChange(of: query) { highlighted = 0 }
            Divider()
            ScrollViewReader { proxy in
                List(Array(entries.enumerated()), id: \.element.id) { i, e in
                    Button { run(e) } label: {
                        HStack { Label(e.title, systemImage: e.icon); Spacer(); Text(e.detail).foregroundStyle(.secondary).font(.caption) }
                            .contentShape(Rectangle())
                    }.buttonStyle(.plain)
                    .listRowBackground(i == highlighted ? Color.accentColor.opacity(0.18) : Color.clear)
                    .id(e.id)
                }
                .onChange(of: highlighted) { if entries.indices.contains(highlighted) { proxy.scrollTo(entries[highlighted].id) } }
            }
            if query.isEmpty && !model.recents.isEmpty {
                Text("Recent first. Type to fuzzy search, ↑↓ to choose, ↩ to run.").font(.caption).foregroundStyle(.secondary).padding(6)
            }
        }
        .frame(width: 560, height: 420)
    }

    private func run(_ e: Entry) {
        model.remember(e.id)
        dismiss()
        e.run(model)
    }
}

/// Exports the page on screen (used by the menu shortcut and the palette; the page's own button uses the same format).
@MainActor enum PageExporter {
    static func export(_ model: AppModel, csv: Bool) {
        guard let p = model.page else { model.error = "Open a list page first."; return }
        let panel = NSSavePanel()
        panel.nameFieldStringValue = ResourceExport.fileName(p.spec.title, ext: csv ? "csv" : "json")
        guard panel.runModal() == .OK, let url = panel.url else { return }
        let text = csv ? ResourceExport.csv(columns: p.columns, rows: p.rows) : ResourceExport.json(rows: p.rows)
        do { try text.write(to: url, atomically: true, encoding: .utf8); model.notice = "Exported \(p.rows.count) rows to \(url.lastPathComponent)" }
        catch { model.error = "Export failed: \(error.localizedDescription)" }
    }
}

/// Multi-select VMs, pick one action, confirm. The confirmation lists the VMs and any that are skipped.
struct BulkActionsSheet: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    @State private var selected: Set<String> = []
    @State private var filter = ""

    private var shown: [VmInfo] { VMFilter(filter).apply(to: model.vms) }
    private var chosen: [VmInfo] { model.vms.filter { selected.contains($0.name) } }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Bulk actions").font(.title2.weight(.semibold))
            HStack {
                TextField("Filter: is:running web vcpus>=4", text: $filter).textFieldStyle(.roundedBorder)
                Button("Select shown") { selected.formUnion(shown.map(\.name)) }
                Button("Clear") { selected = [] }.disabled(selected.isEmpty)
            }
            List(shown) { vm in
                Toggle(isOn: Binding(get: { selected.contains(vm.name) }, set: { if $0 { selected.insert(vm.name) } else { selected.remove(vm.name) } })) {
                    HStack {
                        if model.pins.contains(vm.name) { Image(systemName: "pin.fill").foregroundStyle(.orange).font(.caption) }
                        Text(vm.name); Spacer(); StateBadge(state: vm.state)
                    }
                }
            }.frame(minHeight: 240)
            HStack {
                Text("\(selected.count) selected").foregroundStyle(.secondary).monospacedDigit()
                Spacer()
                Menu("Action") {
                    ForEach(BulkAction.allCases) { a in
                        Button(a.title + " (\(a.plan(chosen).apply.count))", role: a.destructive ? .destructive : nil) { model.bulk(a, on: chosen); dismiss() }
                            .disabled(a.plan(chosen).apply.isEmpty)
                    }
                }.disabled(selected.isEmpty || !model.canWrite).fixedSize()
                Button("Close") { dismiss() }.keyboardShortcut(.cancelAction)
            }
            if !model.canWrite { Text("Your role cannot make changes.").font(.caption).foregroundStyle(.secondary) }
        }
        .padding(22).frame(width: 520, height: 460)
    }
}

struct ShortcutsSheet: View {
    @Environment(\.dismiss) private var dismiss
    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text("Keyboard shortcuts").font(.title2.weight(.semibold))
            ForEach(Array(Dictionary(grouping: ShortcutInfo.all, by: \.group).keys.sorted()), id: \.self) { g in
                VStack(alignment: .leading, spacing: 6) {
                    Text(g).font(.caption.weight(.semibold)).foregroundStyle(.secondary)
                    ForEach(ShortcutInfo.all.filter { $0.group == g }) { s in
                        HStack { Text(s.title); Spacer(); Text(s.keys).font(.system(.body, design: .monospaced)).foregroundStyle(.secondary) }
                    }
                }
            }
            Text("VM filter words: is:running, is:stopped, vcpus>=4, mem>=2048, ip:10.0.").font(.caption).foregroundStyle(.secondary)
            HStack { Spacer(); Button("Done") { dismiss() }.keyboardShortcut(.defaultAction) }
        }
        .padding(22).frame(width: 460)
    }
}

/// App menu commands: every shortcut in the cheat sheet is wired here.
struct MachinaCommands: Commands {
    var model: AppModel
    var body: some Commands {
        CommandMenu("Machina") {
            Button("Command Palette…") { model.showPalette = true }.keyboardShortcut("k", modifiers: .command)
            Divider()
            Button("Fleet") { model.selection = .fleet }.keyboardShortcut("1", modifiers: .command)
            Button("Tasks") { model.selection = .tasks }.keyboardShortcut("2", modifiers: .command)
            Button("Alerts") { model.selection = .alerts }.keyboardShortcut("3", modifiers: .command)
            Divider()
            Button("New VM…") { Task { await model.presentCreateVM() } }.keyboardShortcut("n", modifiers: .command)
            Button("Bulk Actions…") { model.showBulk = true }.keyboardShortcut("b", modifiers: [.command, .shift])
            Button("Pin or Unpin Selected VM") { if case .vm(let n) = model.selection { model.togglePin(n) } }.keyboardShortcut("p", modifiers: [.command, .shift])
            Button("Save VM Filter…") { model.saveCurrentFilter() }.keyboardShortcut("s", modifiers: [.command, .shift])
            Divider()
            Button("Refresh") { Task { await model.refresh(); await model.refreshPlatform() } }.keyboardShortcut("r", modifiers: .command)
            Button("Export Page as CSV…") { PageExporter.export(model, csv: true) }.keyboardShortcut("e", modifiers: [.command, .shift])
            Button("Copy Page Request as curl") { if let p = model.page { model.copyCurl(p.spec.endpoint) } }.keyboardShortcut("c", modifiers: [.command, .shift])
            Divider()
            Button("Keyboard Shortcuts") { model.showShortcuts = true }.keyboardShortcut("/", modifiers: .command)
        }
    }
}
