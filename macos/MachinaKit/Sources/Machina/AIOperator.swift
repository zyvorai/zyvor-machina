import SwiftUI
import MachinaKit

// Conversational AI operator. Model output is untrusted: the plan list only offers allowlisted, role-permitted
// actions (see MachinaKit/AIOperator.swift) and each one runs only after the user approves it, through the same
// confirmation, biometric gate and role gate as a manual action. Prompts are never logged.

@MainActor @Observable
final class AIOperatorModel {
    static let shared = AIOperatorModel()
    static let historyKey = "aiHistoryEnabled"

    struct Turn: Identifiable {
        let id = UUID()
        var message: AIMessage
        var plan: AIPlan?
        var done: Set<Int> = []        // approved or dismissed plan steps
    }

    var turns: [Turn] = []
    var provider: AIProviderState = .unknown
    var busy = false
    var draft = ""
    var historyEnabled: Bool { didSet { UserDefaults.standard.set(historyEnabled, forKey: Self.historyKey); persist() } }
    private let store = AIHistoryStore(url: AIHistoryStore.defaultURL)

    private init() {
        historyEnabled = UserDefaults.standard.object(forKey: Self.historyKey) as? Bool ?? true
        if historyEnabled { turns = store.load().map { Turn(message: $0) } }   // old plans are not restored: approvals are one-shot
    }

    private func persist() { store.save(turns.map(\.message), enabled: historyEnabled) }

    func clearHistory() { turns = []; store.clear() }

    func checkProvider(_ app: AppModel) async {
        guard let c = app.client else { provider = .unknown; return }
        provider = await AIOperator.providerState(c)
    }

    func send(_ app: AppModel) async {
        let text = draft.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty, !busy, let c = app.client else { return }
        draft = ""; busy = true; defer { busy = false }
        turns.append(Turn(message: AIMessage(role: .user, text: text)))
        do {
            let a = try await AIOperator.ask(text, client: c, role: app.role, provider: provider)
            turns.append(Turn(message: AIMessage(role: .assistant, text: a.reply, source: a.source, actionLabels: a.plan.actions.map(\.label)), plan: a.plan))
        } catch {
            turns.append(Turn(message: AIMessage(role: .system, text: error.localizedDescription)))
        }
        persist()
    }

    /// Approves one step: typed phrase (riskiest only), then the standard confirmation dialog, then the write.
    func approve(_ action: AIProposedAction, in turn: Turn, app: AppModel) {
        guard app.canWrite, AIAllowlist.allows(action.policy, role: app.role) else {
            app.error = "\(action.label): your role (\(app.role ?? "unknown")) cannot do that"; return
        }
        let endpoint: Endpoint
        do { endpoint = try AIOperator.executeEndpoint(for: action, role: app.role) } catch { app.error = error.localizedDescription; return }
        let confirmIt: @MainActor () -> Void = { [self] in
            let detail = [action.review, "Risk: \(action.risk)."].filter { !$0.isEmpty }.joined(separator: " ")
            app.confirmAction("Run \"\(action.label)\"?", message: "Proposed by the AI operator. \(detail)", label: "Approve",
                              destructive: action.policy.destructive) { [self] in
                let ok = await app.perform(action.label) { c in
                    let out = try await c.call(endpoint)
                    if let why = ResourceAction.failure(in: out) { throw MachinaError.http(409, why) }
                }
                if ok { mark(action, in: turn.id); app.notice = "\(action.label): done"; await app.refresh() }
            }
        }
        if let phrase = action.policy.typedPhrase {
            app.ask("Confirm \(action.label)", message: "Type \(phrase) to continue.", submit: "Continue",
                    fields: [PromptField(id: "phrase", label: "Type \(phrase)")]) { v in
                if (v["phrase"] ?? "").trimmingCharacters(in: .whitespaces) == phrase { confirmIt() } else { app.error = "Confirmation text did not match; nothing was run." }
            }
        } else { confirmIt() }
    }

    /// Dismisses a step and removes its queued server-side action so it does not linger in the approvals queue.
    func dismiss(_ action: AIProposedAction, in turn: Turn, app: AppModel) {
        mark(action, in: turn.id)
        guard let e = try? AIOperator.rejectEndpoint(for: action, role: app.role), let c = app.client else { return }
        Task { _ = try? await c.call(e) }
    }

    private func mark(_ a: AIProposedAction, in id: UUID) {
        if let i = turns.firstIndex(where: { $0.id == id }) { turns[i].done.insert(a.id) }
    }
}

struct AIOperatorView: View {
    @Environment(AppModel.self) private var app
    @State private var ai = AIOperatorModel.shared
    @FocusState private var focused: Bool

    var body: some View {
        VStack(spacing: 0) {
            header
            Divider()
            if app.role == "readonly" || (app.role != nil && !app.canWrite) {
                ContentUnavailableView("Not available for your role", systemImage: "lock", description: Text("The AI operator needs the operator or admin role."))
            } else if case .unavailable(let why) = ai.provider, ai.turns.isEmpty {
                ContentUnavailableView("AI unavailable", systemImage: "sparkles.slash", description: Text(why))
            } else { transcript }
            Divider()
            composer
        }
        .task { await ai.checkProvider(app) }
        .navigationTitle("AI operator")
        .onAppear { focused = true }
    }

    private var header: some View {
        HStack {
            Image(systemName: "sparkles")
            Text("AI operator").font(.title3.weight(.semibold))
            Spacer()
            Label(ai.provider.label, systemImage: ai.provider.isAvailable ? "checkmark.circle" : "exclamationmark.triangle")
                .font(.caption).foregroundStyle(ai.provider.isAvailable ? Color.secondary : Color.orange)
            Button { Task { await ai.checkProvider(app) } } label: { Image(systemName: "arrow.clockwise") }.help("Check provider")
            Button("Clear", role: .destructive) { ai.clearHistory() }.disabled(ai.turns.isEmpty)
        }.padding(12)
    }

    private var transcript: some View {
        ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 14) {
                    if ai.turns.isEmpty {
                        Text("Ask in plain language, for example \"back up web-1\" or \"create a 20 GiB volume\". The operator only proposes; nothing runs until you approve each action.")
                            .foregroundStyle(.secondary)
                    }
                    ForEach(ai.turns) { turn in
                        bubble(turn).id(turn.id)
                    }
                    if ai.busy { ProgressView("Thinking…").controlSize(.small) }
                }.padding(14).frame(maxWidth: .infinity, alignment: .leading)
            }
            .onChange(of: ai.turns.count) { if let last = ai.turns.last { proxy.scrollTo(last.id, anchor: .bottom) } }
        }
    }

    @ViewBuilder private func bubble(_ turn: AIOperatorModel.Turn) -> some View {
        let m = turn.message
        VStack(alignment: .leading, spacing: 6) {
            Text(m.role == .user ? "You" : m.role == .assistant ? "AI operator" : "Notice").font(.caption.weight(.semibold)).foregroundStyle(.secondary)
            Text(m.text).textSelection(.enabled)       // plain Text: model output is never rendered as markup or links
                .padding(10).frame(maxWidth: .infinity, alignment: .leading)
                .background(m.role == .user ? Color.accentColor.opacity(0.12) : m.role == .system ? Color.orange.opacity(0.12) : Color.secondary.opacity(0.1), in: .rect(cornerRadius: 10))
            if let s = m.source { Text("Answered by: \(s)").font(.caption2).foregroundStyle(.secondary) }
            if let plan = turn.plan { planList(plan, turn) }
            else if m.role == .assistant, !m.actionLabels.isEmpty {
                Text("Earlier plan: \(m.actionLabels.joined(separator: "; ")) (not restorable)").font(.caption).foregroundStyle(.secondary)
            }
        }
    }

    @ViewBuilder private func planList(_ plan: AIPlan, _ turn: AIOperatorModel.Turn) -> some View {
        if !plan.actions.isEmpty {
            VStack(alignment: .leading, spacing: 8) {
                Text("Plan: \(plan.actions.count) proposed action\(plan.actions.count == 1 ? "" : "s")").font(.subheadline.weight(.semibold))
                ForEach(plan.actions) { a in
                    HStack(alignment: .top) {
                        VStack(alignment: .leading, spacing: 2) {
                            Text(a.label).font(.body)
                            if !a.review.isEmpty { Text(a.review).font(.caption).foregroundStyle(.secondary) }
                            Text("Risk: \(a.risk)" + (a.policy.adminOnly ? " · admin only" : "") + (a.policy.destructive ? " · asks to confirm" : ""))
                                .font(.caption2).foregroundStyle(a.risk == "high" || a.risk == "critical" ? Color.red : Color.secondary)
                        }
                        Spacer()
                        if turn.done.contains(a.id) { Label("Handled", systemImage: "checkmark").font(.caption).foregroundStyle(.secondary) }
                        else if a.isExecutable {
                            Button("Dismiss") { ai.dismiss(a, in: turn, app: app) }
                            Button("Approve…") { ai.approve(a, in: turn, app: app) }.buttonStyle(.borderedProminent).disabled(!app.canWrite)
                        } else { Text("Not queued: use Approvals").font(.caption).foregroundStyle(.secondary) }
                    }
                    .padding(8).background(Color.secondary.opacity(0.07), in: .rect(cornerRadius: 8))
                }
            }
        }
        if !plan.dropped.isEmpty {
            Text("\(plan.dropped.count) suggested step\(plan.dropped.count == 1 ? " was" : "s were") not offered: \(Set(plan.dropped.map(\.reason)).sorted().joined(separator: "; ")).")
                .font(.caption).foregroundStyle(.orange)
        }
    }

    private var composer: some View {
        @Bindable var ai = ai
        return HStack(alignment: .bottom) {
            TextField("Ask the AI operator…", text: $ai.draft, axis: .vertical).lineLimit(1...5).textFieldStyle(.roundedBorder)
                .focused($focused).onSubmit { Task { await ai.send(app) } }
            Button { Task { await ai.send(app) } } label: { Image(systemName: "arrow.up.circle.fill").font(.title2) }
                .buttonStyle(.borderless).disabled(ai.busy || ai.draft.trimmingCharacters(in: .whitespaces).isEmpty || !app.canWrite)
                .accessibilityLabel("Send")
        }.padding(12)
    }
}

struct AIOperatorCommands: Commands {
    var model: AppModel
    var body: some Commands {
        CommandGroup(after: .toolbar) {
            Button("AI Operator") { model.selection = .aiOperator }.keyboardShortcut("a", modifiers: [.command, .shift])
        }
    }
}

struct AIOperatorSettingsForm: View {
    @State private var ai = AIOperatorModel.shared
    var body: some View {
        @Bindable var ai = ai
        Form {
            Toggle("Keep conversation history on this Mac", isOn: $ai.historyEnabled)
            Text("Stores only what you typed and the replies, with passwords, tokens and keys removed. Turning this off deletes the saved history.")
                .font(.caption).foregroundStyle(.secondary)
            Button("Clear history now", role: .destructive) { ai.clearHistory() }
        }
    }
}
