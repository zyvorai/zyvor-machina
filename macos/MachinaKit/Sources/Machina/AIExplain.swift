import SwiftUI
import AppKit
import UniformTypeIdentifiers
import MachinaKit

/// "Explain with AI" button: owns its sheet, so adding it to a view is one line. Server text is shown with
/// `Text(verbatim:)` only (no Markdown, no links), and nothing is executed or written to disk unless the user exports.
struct AIExplainButton: View {
    @Environment(AppModel.self) private var model
    let request: AIExplainRequest?
    var compact = false
    @State private var shown = false

    var body: some View {
        Button { shown = true } label: {
            if compact { Image(systemName: "sparkles") } else { Label("Explain", systemImage: "sparkles") }
        }
        .disabled(request == nil || !model.canWrite)
        .help(model.canWrite ? "Explain with AI. Sends only the ids and fields of this item." : "AI explanations need the operator role")
        .accessibilityLabel("Explain with AI")
        .sheet(isPresented: $shown) { if let request { AIExplainSheet(request: request).environment(model) } }
    }
}

enum AIPhase: Equatable { case loading, done(AIExplanation), failed(String, retryable: Bool) }

struct AIExplainSheet: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    let request: AIExplainRequest
    @State private var phase: AIPhase = .loading

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack {
                Label { Text(verbatim: request.title) } icon: { Image(systemName: "sparkles") }.font(.title3.weight(.semibold)).lineLimit(2)
                Spacer()
                Button("Close") { dismiss() }.keyboardShortcut(.cancelAction)
            }
            switch phase {
            case .loading: ProgressView("Asking the AI engine…").frame(maxWidth: .infinity, minHeight: 120)
            case .failed(let msg, let retry):
                ContentUnavailableView {
                    Label("AI unavailable", systemImage: "sparkles.slash")
                } description: { Text(verbatim: msg) } actions: {
                    if retry { Button("Try again") { Task { await load() } } }
                }
            case .done(let e): AIExplanationBody(explanation: e)
            }
            if case .done(let e) = phase {
                HStack {
                    Text("AI output can be wrong. Nothing here runs automatically.").font(.caption).foregroundStyle(.secondary)
                    Spacer()
                    Button("Copy") { copy(plain(e)) }
                    Button("Export Markdown…") { AIExport.save(e.markdown(), name: "explanation.md") }
                }
            }
        }
        .padding(22).frame(minWidth: 560, idealWidth: 640, minHeight: 320)
        .task { await load() }
    }

    private func plain(_ e: AIExplanation) -> String {
        var s = e.text
        for f in e.facts { s += "\n\(f.label): \(f.value)" }
        s += e.steps.enumerated().map { "\n\($0.offset + 1). \($0.element)" }.joined()
        if !e.commands.isEmpty { s += "\n\nCommands:\n" + e.commands.joined(separator: "\n") }
        return s
    }
    private func copy(_ s: String) { NSPasteboard.general.clearContents(); NSPasteboard.general.setString(s, forType: .string) }

    private func load() async {
        phase = .loading
        do {
            let c = try model.api()
            var req = request
            if case .vm(nil, let name) = req { req = .vm(id: await AIResolver.vmID(c, name: name), name: name) }
            phase = .done(try await c.aiExplain(req))
        } catch {
            let f = AIFailure.map(error)
            var retry = true
            if case .forbidden = f { retry = false }
            phase = .failed(f.errorDescription ?? "Failed", retryable: retry)
        }
    }
}

/// The explanation as plain text: facts, read-only steps and commands.
struct AIExplanationBody: View {
    let explanation: AIExplanation
    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 12) {
                if !explanation.text.isEmpty { Text(verbatim: explanation.text).textSelection(.enabled).frame(maxWidth: .infinity, alignment: .leading) }
                ForEach(explanation.facts) { f in
                    VStack(alignment: .leading, spacing: 2) {
                        Text(verbatim: f.label).font(.caption).foregroundStyle(.secondary)
                        Text(verbatim: f.value).textSelection(.enabled)
                    }
                }
                if !explanation.steps.isEmpty {
                    Text("Suggested steps (read only)").font(.headline)
                    ForEach(Array(explanation.steps.enumerated()), id: \.offset) { i, s in
                        HStack(alignment: .firstTextBaseline, spacing: 8) {
                            Text(verbatim: "\(i + 1).").foregroundStyle(.secondary).monospacedDigit()
                            Text(verbatim: s).textSelection(.enabled)
                        }
                    }
                }
                if !explanation.commands.isEmpty {
                    Text("Commands (for reference; run nothing you do not understand)").font(.headline)
                    Text(verbatim: explanation.commands.joined(separator: "\n")).font(.system(.callout, design: .monospaced)).textSelection(.enabled)
                        .padding(8).frame(maxWidth: .infinity, alignment: .leading).background(.quaternary, in: RoundedRectangle(cornerRadius: 6))
                }
            }.frame(maxWidth: .infinity, alignment: .leading)
        }
    }
}

enum AIResolver {
    /// The controller's id for a VM name (`explain` keys on it). nil when the controller does not list it.
    static func vmID(_ c: MachinaClient, name: String) async -> String? {
        guard let v = try? await c.call(.controllerGet("/api/v1/vms")) else { return nil }
        let rows = v.array ?? v["items"]?.array ?? []
        for r in rows where r["name"]?.text == name { if let id = r["id"], case .string(let s) = id { return s } }
        return nil
    }
}

enum AIExport {
    @MainActor static func save(_ markdown: String, name: String) {
        let p = NSSavePanel()
        p.nameFieldStringValue = name
        p.allowedContentTypes = [UTType(filenameExtension: "md") ?? .plainText]
        guard p.runModal() == .OK, let url = p.url else { return }
        try? Data(markdown.utf8).write(to: url, options: .atomic)
    }
}

// MARK: Knowledge page

struct KnowledgeView: View {
    @Environment(AppModel.self) private var model
    enum Mode: String, CaseIterable, Identifiable { case search = "Search", diagnose = "Diagnose", fleet = "Fleet diagnose", runbook = "Runbook", network = "Reachability"
        var id: String { rawValue } }
    @State private var mode: Mode = .search
    @State private var query = ""
    @State private var vmA = ""
    @State private var vmB = ""
    @State private var port = ""
    @State private var busy = false
    @State private var error: String?
    @State private var hits: [KnowledgeHit]?
    @State private var diagnosis: KnowledgeDiagnosis?
    @State private var runbook: KnowledgeRunbook?
    @State private var network: AIExplanation?

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Label("Knowledge", systemImage: "books.vertical").font(.largeTitle.weight(.semibold))
            Picker("", selection: $mode) { ForEach(Mode.allCases) { Text($0.rawValue).tag($0) } }.pickerStyle(.segmented).labelsHidden()
            if mode == .network {
                HStack {
                    Picker("From", selection: $vmA) { Text("Choose…").tag(""); ForEach(model.vms) { Text($0.name).tag($0.name) } }
                    Picker("To", selection: $vmB) { Text("Choose…").tag(""); ForEach(model.vms) { Text($0.name).tag($0.name) } }
                    TextField("Port (optional)", text: $port).frame(width: 120)
                    Button("Explain") { Task { await go() } }.disabled(busy || vmA.isEmpty || vmB.isEmpty)
                }
            } else {
                HStack {
                    TextField(prompt, text: $query).textFieldStyle(.roundedBorder).onSubmit { Task { await go() } }
                    Button(mode == .runbook ? "Generate" : mode == .search ? "Search" : "Diagnose") { Task { await go() } }
                        .disabled(busy || query.trimmingCharacters(in: .whitespaces).isEmpty || !model.canWrite)
                }
            }
            if !model.canWrite { Text("The AI tools need the operator role.").font(.caption).foregroundStyle(.secondary) }
            if busy { ProgressView().frame(maxWidth: .infinity) }
            if let error {
                ContentUnavailableView { Label("AI unavailable", systemImage: "sparkles.slash") } description: { Text(verbatim: error) }
            }
            results
            Spacer(minLength: 0)
        }
        .padding(24)
        .onChange(of: mode) { error = nil }
    }

    private var prompt: String {
        switch mode {
        case .search: "Search runbooks, incidents and docs"
        case .diagnose, .fleet: "Describe the problem (for example: web-1 is slow)"
        default: "Describe the incident to write a runbook for"
        }
    }

    @ViewBuilder private var results: some View {
        switch mode {
        case .search:
            if let hits {
                if hits.isEmpty { ContentUnavailableView.search(text: query) }
                else {
                    List(hits) { h in
                        VStack(alignment: .leading, spacing: 3) {
                            HStack { Text(verbatim: h.title).font(.headline); Spacer(); Text(verbatim: h.kind).font(.caption).foregroundStyle(.secondary) }
                            Text(verbatim: h.snippet).foregroundStyle(.secondary).textSelection(.enabled)
                        }.padding(.vertical, 2)
                    }
                }
            }
        case .diagnose, .fleet:
            if let d = diagnosis {
                HStack { Spacer()
                    Button("Copy") { copy(d.markdown()) }
                    Button("Export Markdown…") { AIExport.save(d.markdown(), name: "diagnosis.md") }
                }
                ScrollView {
                    VStack(alignment: .leading, spacing: 12) {
                        if !d.summary.isEmpty { Text(verbatim: d.summary).textSelection(.enabled) }
                        ForEach(d.extra) { f in VStack(alignment: .leading) { Text(verbatim: f.label).font(.caption).foregroundStyle(.secondary); Text(verbatim: f.value).textSelection(.enabled) } }
                        ForEach(d.hypotheses) { h in
                            VStack(alignment: .leading, spacing: 4) {
                                HStack { Text(verbatim: h.title).font(.headline); Spacer(); Text(verbatim: "\(Int((h.confidence * 100).rounded()))%").monospacedDigit().foregroundStyle(.secondary) }
                                ProgressView(value: h.confidence).accessibilityHidden(true)
                                if !h.evidence.isEmpty { Text(verbatim: h.evidence).textSelection(.enabled) }
                                if !h.action.isEmpty { Text(verbatim: "Suggested: " + h.action).foregroundStyle(.secondary).textSelection(.enabled) }
                            }.padding(10).background(.quaternary.opacity(0.5), in: RoundedRectangle(cornerRadius: 8))
                        }
                    }.frame(maxWidth: .infinity, alignment: .leading)
                }
            }
        case .runbook:
            if let r = runbook {
                HStack { Text(verbatim: r.title).font(.title3.weight(.semibold)); Spacer()
                    Button("Copy") { copy(r.plain()) }
                    Button("Export Markdown…") { AIExport.save(r.markdown(), name: "runbook.md") }
                }
                AIExplanationBody(explanation: AIExplanation(title: r.title, text: r.summary, steps: r.steps, commands: r.commands))
            }
        case .network:
            if let n = network {
                HStack { Spacer()
                    Button("Copy") { copy(n.markdown()) }
                    Button("Export Markdown…") { AIExport.save(n.markdown(), name: "reachability.md") }
                }
                AIExplanationBody(explanation: n)
            }
        }
    }

    private func copy(_ s: String) { NSPasteboard.general.clearContents(); NSPasteboard.general.setString(s, forType: .string) }

    private func go() async {
        busy = true; error = nil
        defer { busy = false }
        do {
            let c = try model.api()
            switch mode {
            case .search: hits = try await c.knowledgeSearch(query)
            case .diagnose: diagnosis = try await c.knowledgeDiagnose(query)
            case .fleet: diagnosis = try await c.knowledgeDiagnose(query, fleet: true)
            case .runbook: runbook = try await c.knowledgeRunbook(query)
            case .network: network = try await c.aiExplain(.network(vmA: vmA, vmB: vmB, port: Int(port.trimmingCharacters(in: .whitespaces)).flatMap { (1...65535).contains($0) ? $0 : nil }))
            }
        } catch { self.error = AIFailure.map(error).errorDescription }
    }
}
