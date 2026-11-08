import SwiftUI
import MachinaKit

struct PendingAction: Identifiable {
    let id = UUID()
    var title: String
    var message: String
    var label: String
    var destructive = true
    var run: @MainActor () async -> Void
}

struct PromptField: Identifiable {
    var id: String
    var label: String
    var initial = ""
    var options: [String]?      // nil = free text, otherwise a picker
    var multiline = false       // a monospaced text editor (JSON documents)
}

struct PromptRequest: Identifiable {
    let id = UUID()
    var title: String
    var message: String
    var submit: String
    var fields: [PromptField]
    var run: @MainActor ([String: String]) async -> Void
}

extension View {
    func confirm(_ pending: Binding<PendingAction?>) -> some View {
        confirmationDialog(pending.wrappedValue?.title ?? "",
                           isPresented: Binding(get: { pending.wrappedValue != nil }, set: { if !$0 { pending.wrappedValue = nil } }),
                           titleVisibility: .visible, presenting: pending.wrappedValue) { p in
            Button(p.label, role: p.destructive ? .destructive : nil) { Task { await p.run() } }
            Button("Cancel", role: .cancel) {}
        } message: { p in Text(p.message) }
    }

    func prompt(_ request: Binding<PromptRequest?>) -> some View {
        sheet(item: request) { PromptSheet(request: $0) }
    }
}

struct PromptSheet: View {
    @Environment(\.dismiss) private var dismiss
    let request: PromptRequest
    @State private var values: [String: String] = [:]

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text(request.title).font(.title2.weight(.semibold))
            if !request.message.isEmpty { Text(request.message).foregroundStyle(.secondary).font(.callout) }
            ForEach(request.fields) { f in
                if let options = f.options {
                    Picker(f.label, selection: binding(f)) { ForEach(options, id: \.self) { Text($0).tag($0) } }
                } else if f.multiline {
                    VStack(alignment: .leading, spacing: 4) {
                        Text(f.label).font(.callout).foregroundStyle(.secondary)
                        TextEditor(text: binding(f)).font(.system(.callout, design: .monospaced)).autocorrectionDisabled()
                            .frame(minHeight: 200).overlay(RoundedRectangle(cornerRadius: 6).stroke(.quaternary))
                    }
                } else {
                    TextField(f.label, text: binding(f))
                }
            }
            HStack {
                Spacer()
                Button("Cancel", role: .cancel) { dismiss() }.keyboardShortcut(.cancelAction)
                Button(request.submit) {
                    let v = values
                    dismiss()
                    Task { await request.run(v) }
                }.keyboardShortcut(.defaultAction).buttonStyle(.borderedProminent)
            }
        }
        .padding(22).frame(width: request.fields.contains { $0.multiline } ? 580 : 440)
        .onAppear { for f in request.fields { values[f.id] = f.initial.isEmpty ? (f.options?.first ?? "") : f.initial } }
    }

    private func binding(_ f: PromptField) -> Binding<String> {
        Binding(get: { values[f.id] ?? "" }, set: { values[f.id] = $0 })
    }
}
