import SwiftUI
import AppKit
import MachinaKit

private final class ResultBox: @unchecked Sendable { var value: JSONValue = .null }

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
                if let create = spec.create {
                    Button("New…") { openCreate(create) }.disabled(!model.canWrite).help(model.canWrite ? "" : "Your role cannot make changes")
                }
                ForEach(spec.actions) { a in
                    Button(a.title) { run(a) }
                        .disabled((selected == nil && !a.global) || !model.canWrite)
                        .help(model.canWrite ? "" : "Your role cannot make changes")
                }
                Menu {
                    Button("Export CSV…") { exportPage(csv: true) }
                    Button("Export JSON…") { exportPage(csv: false) }
                    Divider()
                    Button("Copy as curl") { model.copyCurl(spec.endpoint, "curl for \(spec.title)") }
                    if let r = selected {
                        ForEach(spec.actions.filter { $0.endpoint(for: r) != nil }) { act in
                            Button("Copy curl: \(act.title) selected row") { if let e = act.endpoint(for: r) { model.copyCurl(e, "curl for \(act.title)") } }
                        }
                    }
                } label: { Image(systemName: "square.and.arrow.up") }.menuStyle(.borderlessButton).fixedSize().help("Export or copy as curl")
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
        .onChange(of: rows) { model.page = PageContext(spec: spec, columns: columns, rows: filtered) }
        .onChange(of: search) { model.page = PageContext(spec: spec, columns: columns, rows: filtered) }
        .onDisappear { if model.page?.spec.id == spec.id { model.page = nil } }
        .task(id: spec.id) {
            await load()
            while !Task.isCancelled {
                try? await Task.sleep(nanoseconds: 15_000_000_000)
                if Task.isCancelled { break }
                await load()
            }
        }
    }

    /// Saves the rows currently shown (after the filter) with a save panel; JSON keeps every field, CSV the visible columns.
    private func exportPage(csv: Bool) {
        let panel = NSSavePanel()
        panel.nameFieldStringValue = ResourceExport.fileName(spec.title, ext: csv ? "csv" : "json")
        guard panel.runModal() == .OK, let url = panel.url else { return }
        let text = csv ? ResourceExport.csv(columns: columns, rows: filtered) : ResourceExport.json(rows: filtered)
        do { try text.write(to: url, atomically: true, encoding: .utf8); model.notice = "Exported \(filtered.count) rows to \(url.lastPathComponent)" }
        catch { model.error = "Export failed: \(error.localizedDescription)" }
    }

    private func openCreate(_ c: ResourceCreate) {
        Task {
            // Pickers are filled from the server first; a failed lookup leaves that field empty (and the form refuses to submit if it is required).
            var labels: [String: [String: String]] = [:]   // field id -> option label -> value
            var fields: [PromptField] = []
            for f in c.fields {
                if let ch = f.choices {
                    let json = (try? await model.api().call(ch.endpoint)) ?? .null
                    var opts = ch.options(from: json)
                    if opts.isEmpty && f.required { model.error = "\(c.title): no \(f.label.lowercased()) available."; return }
                    if !f.required { opts.insert(("none", ""), at: 0) }
                    labels[f.id] = Dictionary(opts.map { ($0.label, $0.value) }, uniquingKeysWith: { a, _ in a })
                    fields.append(PromptField(id: f.id, label: f.label, initial: opts.first?.label ?? "", options: opts.map(\.label)))
                } else {
                    fields.append(PromptField(id: f.id, label: f.label, initial: f.initial, options: f.kind == .bool ? ["yes", "no"] : nil))
                }
            }
            model.ask(c.title, submit: "Create", fields: fields) { values in
                var values = values
                for (id, map) in labels { values[id] = map[values[id] ?? ""] ?? "" }
                let body: JSONValue
                do { body = try c.body(from: values) } catch { model.error = error.localizedDescription; return }
                var draft = c.endpoint; draft.body = body
                let e = draft
                if await model.perform(c.title, { _ = try await $0.call(e) }) { model.notice = "\(c.title): queued"; await load() }
            }
        }
    }

    /// One row action: ask for its inputs, confirm if destructive or flagged, run, report `ok: false` bodies, reload.
    private func run(_ a: ResourceAction) {
        let row = selected ?? ResourceRow(id: "", values: .object([:]))
        guard a.global || selected != nil, a.endpoint(for: row) != nil else { model.error = "\(a.title): the row is missing a value the request needs."; return }
        let target = a.global ? spec.title : (columns.first.map { $0.render(row) } ?? row.id)
        let execute: @MainActor ([String: JSONValue]) async -> Void = { extra in
            guard let endpoint = a.endpoint(for: row, inputValues: extra) else { return }
            let box = ResultBox()
            let ok = await model.perform("\(a.title) \(target)", { box.value = try await $0.call(endpoint) })
            guard ok else { return }
            let outcome = box.value
            if let why = ResourceAction.failure(in: outcome) { model.error = "\(a.title) \(target): \(why)"; return }
            model.notice = ResourceAction.summary(of: outcome).map { "\(a.title): \($0)" } ?? "\(a.title): done"
            await load()
        }
        let confirmThenRun: @MainActor ([String: JSONValue]) -> Void = { extra in
            if a.destructive || a.confirm != nil {
                model.confirmAction("\(a.title) \(target)?", message: a.confirm ?? "This changes the server and may not be undoable.", label: a.title, destructive: a.destructive) { await execute(extra) }
            } else { Task { await execute(extra) } }
        }
        if a.inputs.isEmpty { confirmThenRun([:]); return }
        let fields = a.inputs.map { PromptField(id: $0.id, label: $0.label, initial: $0.initial, options: $0.kind == .bool ? ["yes", "no"] : nil) }
        model.ask("\(a.title) \(target)", submit: "Continue", fields: fields) { values in
            do { confirmThenRun(try a.inputBody(from: values)) } catch { model.error = error.localizedDescription }
        }
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
