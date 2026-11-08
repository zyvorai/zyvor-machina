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
    @State private var wizard = false
    @State private var result: ResultText?

    private struct ResultText: Identifiable { let id = UUID(); var title: String; var text: String }

    private var columns: [ResourceColumn] { spec.effectiveColumns(for: rows) }
    private var filtered: [ResourceRow] {
        search.isEmpty ? rows : rows.filter { r in columns.contains { $0.render(r).localizedCaseInsensitiveContains(search) } }
    }
    private var selected: ResourceRow? { rows.first { $0.id == selection } }

    var body: some View {
        if wizard { InstanceWizard { wizard = false; Task { await load() } } }
        else { page }
    }

    private var page: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Label(spec.title, systemImage: spec.icon).font(.largeTitle.weight(.semibold))
                Spacer()
                Text("\(filtered.count) of \(rows.count)").foregroundStyle(.secondary).monospacedDigit()
                Button { Task { await load() } } label: { Image(systemName: "arrow.clockwise") }
                if spec.accessory == .isoTransfer {
                    Button("Upload…") { model.pickISOsToUpload() }.disabled(!model.canWrite).help(model.canWrite ? "Or drop .iso files anywhere on the window" : "Your role cannot make changes")
                    Button("Download from URL…") { model.promptISODownload() }.disabled(!model.canWrite)
                }
                if spec.accessory == .instanceWizard {
                    Button("Launch…") { wizard = true }.disabled(!model.canWrite).help(model.canWrite ? "" : "Your role cannot make changes")
                }
                if let create = spec.create {
                    Button("New…") { openCreate(create) }.disabled(!model.canWrite).help(model.canWrite ? "" : "Your role cannot make changes")
                }
                ForEach(spec.actions) { a in
                    Button(a.title) { run(a) }
                        .disabled(selected == nil || !model.canWrite)
                        .help(model.canWrite ? "" : "Your role cannot make changes")
                }
                if spec.layout == .table {
                    Toggle(isOn: $inspect) { Image(systemName: "sidebar.right") }.toggleStyle(.button).help("Show details")
                }
            }
            PlatformErrorBanner(text: loadError)
            if spec.accessory == .isoTransfer { TransferList() }
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
        .sheet(item: $result) { r in
            VStack(alignment: .leading, spacing: 12) {
                Text(r.title).font(.title2.weight(.semibold))
                ScrollView { Text(r.text).font(.system(.callout, design: .monospaced)).textSelection(.enabled).frame(maxWidth: .infinity, alignment: .leading) }
                    .padding(8).background(.quaternary.opacity(0.4), in: .rect(cornerRadius: 8))
                HStack { Spacer(); Button("Done") { result = nil }.keyboardShortcut(.defaultAction) }
            }.padding(22).frame(width: 640, height: 480)
        }
        .onAppear { if spec.accessory == .isoTransfer { model.transfers.onFinished = { Task { await load() } } } }
        .task(id: spec.id) {
            await load()
            while !Task.isCancelled {
                try? await Task.sleep(nanoseconds: 15_000_000_000)
                if Task.isCancelled { break }
                await load()
            }
        }
    }

    private static func promptFields(_ fields: [ResourceCreate.Field], initial: [String: String] = [:]) -> [PromptField] {
        fields.map { f in
            var options: [String]?
            switch f.kind { case .bool: options = ["yes", "no"]; case .choice(let o): options = o; default: break }
            return PromptField(id: f.id, label: f.label, initial: initial[f.id] ?? f.initial, options: options, multiline: f.kind == .json)
        }
    }

    private func openCreate(_ c: ResourceCreate) {
        model.ask(c.title, submit: "Create", fields: Self.promptFields(c.fields)) { values in
            let body: JSONValue
            do { body = try c.body(from: values) } catch { model.error = error.localizedDescription; return }
            var draft = c.endpoint; draft.body = body
            let e = draft
            model.confirmAction("\(c.title)?", message: "\(e.method) \(e.path)", label: "Create", destructive: false) {
                if await model.perform(c.title, { _ = try await $0.call(e) }) { model.notice = "\(c.title): done"; await load() }
            }
        }
    }

    /// One row action: ask for its input (if any), build the request from the selected row, confirm, run, reload.
    private func run(_ a: ResourceAction) {
        guard let row = selected, a.endpoint(for: row) != nil else { model.error = "\(a.title): the row is missing a value the request needs."; return }
        let target = columns.first.map { $0.render(row) } ?? row.id
        let go: @MainActor ([String: String]) -> Void = { values in
            let endpoint: Endpoint
            do { guard let e = try a.endpoint(for: row, input: values) else { model.error = "\(a.title): the row is missing a value the request needs."; return }; endpoint = e }
            catch { model.error = error.localizedDescription; return }
            if a.showsResult {      // a read-only preview (stack plan): no write, so no confirmation
                Task {
                    do { result = ResultText(title: "\(a.title) \(target)", text: try await model.call(endpoint).pretty); model.error = nil }
                    catch { model.error = "\(a.title): \(error.localizedDescription)" }
                }
                return
            }
            let exec: @MainActor () async -> Void = {
                if await model.perform("\(a.title) \(target)", { _ = try await $0.call(endpoint) }) { model.notice = "\(a.title): done"; await load() }
            }
            model.confirmAction("\(a.title) \(target)?", message: a.destructive ? "This changes the server and may not be undoable." : "\(endpoint.method) \(endpoint.path)",
                                label: a.title.replacingOccurrences(of: "…", with: ""), destructive: a.destructive, run: exec)
        }
        if a.input.isEmpty { go([:]) }
        else { model.ask("\(a.title) \(target)", submit: "Continue", fields: Self.promptFields(a.input, initial: a.initialValues(for: row))) { go($0) } }
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
