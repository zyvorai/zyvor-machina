import SwiftUI
import MachinaKit

/// Renders any `ResourceSpec`: loads its endpoint, refreshes every 15 s, filters, and shows the
/// selected row's raw fields in an inspector.
struct ResourceListView: View {
    @Environment(AppModel.self) private var model
    let spec: ResourceSpec

    @State private var rows: [ResourceRow] = []
    @State private var loaded = false
    @State private var loadError: String?
    @State private var search = ""
    @State private var selection: ResourceRow.ID?
    @State private var inspect = false

    private var columns: [ResourceColumn] { spec.effectiveColumns(for: rows) }
    private var filtered: [ResourceRow] {
        search.isEmpty ? rows : rows.filter { r in columns.contains { $0.render(r).localizedCaseInsensitiveContains(search) } }
    }
    private var selected: ResourceRow? { rows.first { $0.id == selection } }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Label(spec.title, systemImage: spec.icon).font(.largeTitle.weight(.semibold))
                Spacer()
                Text("\(filtered.count) of \(rows.count)").foregroundStyle(.secondary).monospacedDigit()
                Button { Task { await load() } } label: { Image(systemName: "arrow.clockwise") }
                ForEach(spec.allCreates, id: \.title) { c in
                    Button(spec.create == c && c.title == "New…" ? "New…" : c.title) { openCreate(c) }
                        .disabled(!allowed(admin: c.adminOnly)).help(roleHelp(admin: c.adminOnly))
                }
                ForEach(spec.actions) { a in
                    Button(a.title) { run(a) }
                        .disabled((selected == nil && !a.rowless) || !allowed(admin: a.adminOnly))
                        .help(roleHelp(admin: a.adminOnly))
                }
                if spec.layout == .table {
                    Toggle(isOn: $inspect) { Image(systemName: "sidebar.right") }.toggleStyle(.button).help("Show details")
                }
            }
            PlatformErrorBanner(text: loadError)
            if !loaded {
                ProgressView().frame(maxWidth: .infinity, maxHeight: .infinity)
            } else if rows.isEmpty {
                ContentUnavailableView("Nothing here", systemImage: spec.icon, description: Text("The server returned no rows."))
            } else {
                Table(of: ResourceRow.self, selection: $selection) {
                    TableColumnForEach(columns) { col in
                        TableColumn(col.title) { row in Text(col.render(row)).textSelection(.enabled).lineLimit(2) }
                    }
                } rows: {
                    ForEach(filtered) { TableRow($0) }
                }
            }
        }
        .padding(24)
        .searchable(text: $search, prompt: "Filter \(spec.title.lowercased())")
        .inspector(isPresented: $inspect) {
            if let r = selected, let o = r.values.object {
                List(o.keys.sorted(), id: \.self) { k in
                    VStack(alignment: .leading, spacing: 2) {
                        Text(k).font(.caption).foregroundStyle(.secondary)
                        Text(o[k]!.text).textSelection(.enabled)
                    }
                }.inspectorColumnWidth(min: 240, ideal: 300)
            } else {
                ContentUnavailableView("Select a row", systemImage: "cursorarrow.click").inspectorColumnWidth(min: 240, ideal: 300)
            }
        }
        .task(id: spec.id) {
            await load()
            while !Task.isCancelled, spec.refreshSecs > 0 {
                try? await Task.sleep(nanoseconds: UInt64(spec.refreshSecs) * 1_000_000_000)
                if Task.isCancelled { break }
                await load()
            }
        }
    }

    private func allowed(admin: Bool) -> Bool { admin ? model.isAdmin : model.canWrite }
    private func roleHelp(admin: Bool) -> String {
        allowed(admin: admin) ? "" : (admin ? "Needs the admin role" : "Your role cannot make changes")
    }

    private func openCreate(_ c: ResourceCreate) {
        var fields = c.fields.map { f in
            PromptField(id: f.id, label: f.label, initial: f.initial, options: f.kind == .bool ? ["yes", "no"] : (f.options.isEmpty ? nil : f.options))
        }
        if let phrase = c.typedPhrase { fields.append(PromptField(id: ResourceCreate.phraseKey, label: "Type \(phrase) to confirm")) }
        model.ask(c.title, message: c.warning ?? "", submit: "Continue", fields: fields) { values in
            let body: JSONValue
            do { body = try c.body(from: values) } catch { model.error = error.localizedDescription; return }
            var draft = c.endpoint; draft.body = body
            let e = draft
            let send: @MainActor () async -> Void = {
                let ok = await model.perform(c.title, { client in
                    let answer = try await client.call(e)
                    if let key = c.resultKey, let text = answer[key]?.text {
                        await MainActor.run { model.textResult = TextResult(title: c.title, text: text) }
                    }
                })
                if ok { model.notice = "\(c.title): done"; await load() }
            }
            model.confirmAction("\(c.title)?", message: c.warning ?? "This changes the server.", label: "Send", destructive: true, run: send)
        }
    }

    /// One row action: build the request from the selected row, confirm if it is destructive, run, reload.
    private func run(_ a: ResourceAction) {
        let row = a.rowless ? ResourceRow(id: "", values: .object([:])) : selected
        guard let row, let endpoint = a.endpoint(for: row) else { model.error = "\(a.title): the row is missing a value the request needs."; return }
        let target = a.rowless ? spec.title : (columns.first.map { $0.render(row) } ?? row.id)
        let go: @MainActor () async -> Void = {
            if await model.perform("\(a.title) \(target)", { _ = try await $0.call(endpoint) }) { model.notice = "\(a.title): done"; await load() }
        }
        let confirm: @MainActor () -> Void = {
            model.confirmAction("\(a.title) \(target)?", message: a.confirm ?? "This changes the server and may not be undoable.", label: a.title, destructive: a.destructive, run: go)
        }
        if let phrase = a.phrase(for: row) {
            model.ask("\(a.title) \(target)", message: (a.confirm ?? "") + "\n\nType \(phrase) to continue.", submit: "Continue", fields: [PromptField(id: "typed", label: "Type \(phrase)")]) { values in
                guard (values["typed"] ?? "").trimmingCharacters(in: .whitespaces) == phrase else { model.error = "\(a.title): the typed name did not match, nothing was sent."; return }
                confirm()
            }
        } else { confirm() }
    }

    private func load() async {
        do {
            rows = spec.rows(from: try await model.load(spec))
            loadError = nil
        } catch {
            loadError = error.localizedDescription
        }
        loaded = true
    }
}

/// Cmd-K: jump to any page, fleet view or VM.
struct CommandPalette: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    @State private var query = ""

    private struct Entry: Identifiable { var id: String; var title: String; var detail: String; var icon: String; var item: SidebarItem }

    private var entries: [Entry] {
        var e: [Entry] = [
            .init(id: "fleet", title: "Fleet", detail: "Overview", icon: "server.rack", item: .fleet),
            .init(id: "tasks", title: "Tasks", detail: "Overview", icon: "checklist", item: .tasks),
            .init(id: "alerts", title: "Alerts", detail: "Overview", icon: "bell", item: .alerts),
        ]
        e += FeatureRegistry.all.map { .init(id: "p-" + $0.id, title: $0.title, detail: $0.group, icon: $0.icon, item: .page($0.id)) }
        e += model.vms.map { .init(id: "vm-" + $0.name, title: $0.name, detail: "VM · \($0.state)", icon: "desktopcomputer", item: .vm($0.name)) }
        return query.isEmpty ? e : e.filter { $0.title.localizedCaseInsensitiveContains(query) || $0.detail.localizedCaseInsensitiveContains(query) }
    }

    var body: some View {
        VStack(spacing: 0) {
            TextField("Go to…", text: $query).textFieldStyle(.plain).font(.title3).padding(14)
                .onSubmit { if let f = entries.first { go(f.item) } }
            Divider()
            List(entries) { e in
                Button { go(e.item) } label: {
                    HStack { Label(e.title, systemImage: e.icon); Spacer(); Text(e.detail).foregroundStyle(.secondary).font(.caption) }
                }.buttonStyle(.plain)
            }
        }
        .frame(width: 520, height: 380)
    }

    private func go(_ item: SidebarItem) { model.selection = item; dismiss() }
}
