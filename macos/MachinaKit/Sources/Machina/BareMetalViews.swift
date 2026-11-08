import SwiftUI
import MachinaKit

/// Serial-over-LAN for an IPMI bare-metal server. Admin only; asks for a single-use token (audited by the controller)
/// each time it connects. The WebSocket goes through the daemon's proxy, so the saved certificate pin applies.
struct SolConsoleSheet: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    let row: ResourceRow
    @State private var sol = SerialConsoleModel()

    private var name: String { row.values["hostname"]?.text ?? row.id }
    private var blocker: String? { model.isAdmin ? BareMetalOps.solBlocker(row: row) : BareMetalError.notAdmin.errorDescription }

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack {
                Label("Serial console: \(name)", systemImage: "terminal").font(.title2.weight(.semibold))
                Spacer()
                if sol.state == .connected { Button("Disconnect") { sol.disconnect() } }
                else { Button("Connect") { connect() }.buttonStyle(.borderedProminent).disabled(blocker != nil || sol.state == .connecting) }
                Button("Done") { sol.disconnect(); dismiss() }.keyboardShortcut(.cancelAction)
            }
            if let b = blocker { Text(b).foregroundStyle(.secondary) }
            ZStack {
                SerialSurface(model: sol)
                switch sol.state {
                case .connecting: Text("Connecting…").foregroundStyle(.white.opacity(0.8))
                case .failed(let m): Text(m).foregroundStyle(.white.opacity(0.8)).multilineTextAlignment(.center).padding()
                case .idle: if blocker == nil { Text("Not connected").foregroundStyle(.white.opacity(0.8)) }
                case .connected: EmptyView()
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity).background(.black).clipShape(.rect(cornerRadius: 8))
            Text("Keystrokes go to the machine's console as typed. Only one SOL session per BMC can be active; connecting ends a stale one. The BMC password never reaches this app.")
                .font(.caption).foregroundStyle(.secondary)
        }
        .padding(22).frame(minWidth: 760, minHeight: 520)
        .onDisappear { sol.disconnect() }
    }

    private func connect() {
        guard blocker == nil, let api = try? model.api() else { return }
        let id = row.id
        sol.connect { try await api.openSol(serverId: id) }
    }
}
