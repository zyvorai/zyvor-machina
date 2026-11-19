import SwiftUI
import MachinaKit

/// A small status line at the bottom of the main window. Informational only: it never blocks or disables anything.
struct LicenceStatusBar: View {
    private let licence = LicenceManager.shared
    @Environment(\.openWindow) private var openWindow

    var body: some View {
        let s = licence.state
        VStack(spacing: 0) {
            Divider()
            HStack(spacing: 8) {
                Label(s.statusText, systemImage: s.needsAttention ? "exclamationmark.triangle.fill" : "key")
                    .font(.caption).foregroundStyle(s.needsAttention ? .orange : .secondary)
                if s.showsTrialNote {
                    Text("Free 30-day trial key:").font(.caption).foregroundStyle(.secondary)
                    Link("sales@zyvor.dev", destination: LicenceState.trialURL).font(.caption)
                }
                Spacer()
                if s.showsBuyLink { Link("Get a licence", destination: LicenceState.buyURL).font(.caption) }
                Button("Licence…") { openWindow(id: "licence") }.buttonStyle(.link).font(.caption)
            }
            .padding(.horizontal, 12).padding(.vertical, 5)
            .background(.bar)
        }
    }
}

/// Machina > Licence…: paste a key, verify and save it, see what it grants, remove it.
struct LicenceView: View {
    private let licence = LicenceManager.shared
    @State private var pasted = ""
    @State private var message: String?
    @State private var saved = false
    @State private var confirmRemove = false

    var body: some View {
        let s = licence.state
        VStack(alignment: .leading, spacing: 14) {
            Text("Licence").font(.title2.bold())
            Label(s.statusText, systemImage: s.needsAttention ? "exclamationmark.triangle.fill" : "key")
                .foregroundStyle(s.needsAttention ? .orange : .primary)
            if let c = s.claims {
                Grid(alignment: .leading, horizontalSpacing: 14, verticalSpacing: 4) {
                    GridRow { Text("Licensee").foregroundStyle(.secondary); Text(c.licensee).textSelection(.enabled) }
                    GridRow { Text("Edition").foregroundStyle(.secondary); Text(s.editionName ?? "") }
                    GridRow { Text("Expires").foregroundStyle(.secondary); Text(LicenceState.dateText(c.expires)) }
                    GridRow { Text("Days left").foregroundStyle(.secondary); Text(daysLeft(s)) }
                }
            }
            GroupBox {
                VStack(alignment: .leading, spacing: 8) {
                    Text(s.claims == nil ? "Paste your licence key" : "Replace the key").font(.headline)
                    TextEditor(text: $pasted).font(.system(.caption, design: .monospaced)).frame(height: 84)
                        .overlay(RoundedRectangle(cornerRadius: 6).stroke(.quaternary))
                    HStack {
                        if let message { Label(message, systemImage: saved ? "checkmark.circle" : "xmark.octagon").font(.caption).foregroundStyle(saved ? .green : .red) }
                        Spacer()
                        Button("Verify and Save") { verify() }.buttonStyle(.borderedProminent).keyboardShortcut(.defaultAction)
                            .disabled(pasted.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                    }
                    Text("Keys look like ZYV1.… and are checked on this Mac. Nothing is sent anywhere.").font(.caption).foregroundStyle(.secondary)
                }.padding(4)
            }
            HStack {
                Link("Get a licence", destination: LicenceState.buyURL)
                Link("Free 30-day trial key: sales@zyvor.dev", destination: LicenceState.trialURL)
                Spacer()
                Button("Remove Key…", role: .destructive) { confirmRemove = true }.disabled(s == .unlicensed)
            }.font(.callout)
            Text("Machina is free for evaluation and non-production use. Production use needs a licence; Machina keeps working without one.").font(.caption).foregroundStyle(.secondary)
        }
        .padding(22).frame(width: 520)
        .confirmationDialog("Remove the licence key from this Mac?", isPresented: $confirmRemove) {
            Button("Remove Key", role: .destructive) { licence.remove(); message = nil; pasted = "" }
        } message: { Text("Machina keeps working; it will show as unlicensed.") }
    }

    private func daysLeft(_ s: LicenceState) -> String {
        switch s { case .trial(let n, _), .pro(let n, _): return "\(n)"; default: return "0 (expired)" }
    }

    private func verify() {
        if let err = licence.activate(pasted) { message = err; saved = false }
        else { message = "Saved. " + licence.state.statusText; saved = true; pasted = "" }
    }
}

/// The Licence tab in Settings.
struct LicenceSettingsForm: View {
    private let licence = LicenceManager.shared
    @Environment(\.openWindow) private var openWindow
    var body: some View {
        let s = licence.state
        Form {
            HStack {
                Label(s.statusText, systemImage: s.needsAttention ? "exclamationmark.triangle.fill" : "key")
                Spacer()
                Button("Manage Licence…") { openWindow(id: "licence") }
            }
            if s.showsBuyLink || s.showsTrialNote {
                HStack(spacing: 12) {
                    Link("Get a licence", destination: LicenceState.buyURL)
                    if s.showsTrialNote { Link("Free 30-day trial key: sales@zyvor.dev", destination: LicenceState.trialURL) }
                }.font(.caption)
            }
        }
    }
}

struct LicenceCommands: Commands {
    @Environment(\.openWindow) private var openWindow
    var body: some Commands {
        CommandGroup(after: .appInfo) {
            Button("Licence…") { openWindow(id: "licence") }
        }
    }
}
