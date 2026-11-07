import AppIntents
import Foundation
import MachinaKit

/// Shortcuts and Siri. They run in the app's own process, so they use the signed-in session; without one they
/// ask the user to open Machina.
struct IntentFailure: Error, CustomLocalizedStringResourceConvertible {
    var message: String
    var localizedStringResource: LocalizedStringResource { LocalizedStringResource(stringLiteral: message) }
}

@MainActor private func readySession() throws -> AppModel {
    let m = AppModel.shared
    guard m.phase == .ready else { throw IntentFailure(message: "Open Machina and sign in first.") }
    return m
}

struct VMEntity: AppEntity {
    static let typeDisplayRepresentation: TypeDisplayRepresentation = "Virtual Machine"
    static let defaultQuery = VMQuery()
    var id: String
    var displayRepresentation: DisplayRepresentation { DisplayRepresentation(title: "\(id)") }
}

struct VMQuery: EntityQuery {
    func entities(for identifiers: [String]) async throws -> [VMEntity] {
        let names = await MainActor.run { Set(AppModel.shared.vms.map(\.name)) }
        return identifiers.filter(names.contains).map(VMEntity.init)
    }
    func suggestedEntities() async throws -> [VMEntity] { await MainActor.run { AppModel.shared.vms.map { VMEntity(id: $0.name) } } }
}

@MainActor private func power(_ action: PowerAction, _ vm: VMEntity) async throws {
    let m = try readySession()
    guard let info = m.vms.first(where: { $0.name == vm.id }) else { throw IntentFailure(message: "No VM named \(vm.id).") }
    guard await m.perform("\(action.rawValue.capitalized) \(vm.id)", { try await $0.power(action, vm: info) }) else {
        throw IntentFailure(message: m.error ?? "The action failed.")
    }
}

struct StartVMIntent: AppIntent {
    static let title: LocalizedStringResource = "Start VM"
    static let description = IntentDescription("Starts a virtual machine.")
    @Parameter(title: "VM") var vm: VMEntity
    @MainActor func perform() async throws -> some IntentResult { try await power(.start, vm); return .result() }
}

struct ShutDownVMIntent: AppIntent {
    static let title: LocalizedStringResource = "Shut Down VM"
    static let description = IntentDescription("Asks a virtual machine to shut down cleanly.")
    @Parameter(title: "VM") var vm: VMEntity
    @MainActor func perform() async throws -> some IntentResult { try await power(.shutdown, vm); return .result() }
}

struct RebootVMIntent: AppIntent {
    static let title: LocalizedStringResource = "Reboot VM"
    static let description = IntentDescription("Reboots a virtual machine.")
    @Parameter(title: "VM") var vm: VMEntity
    @MainActor func perform() async throws -> some IntentResult { try await power(.reboot, vm); return .result() }
}

struct SnapshotVMIntent: AppIntent {
    static let title: LocalizedStringResource = "Take VM Snapshot"
    static let description = IntentDescription("Takes a snapshot of a virtual machine.")
    @Parameter(title: "VM") var vm: VMEntity
    @Parameter(title: "Name", default: "") var name: String
    @MainActor func perform() async throws -> some IntentResult {
        let m = try readySession()
        guard let info = m.vms.first(where: { $0.name == vm.id }) else { throw IntentFailure(message: "No VM named \(vm.id).") }
        let snap = name.isEmpty ? "shortcut-\(Int(Date().timeIntervalSince1970))" : name
        guard await m.perform("Snapshot \(vm.id)", { try await $0.createSnapshot(info, name: snap, description: "Taken from Shortcuts") }) else {
            throw IntentFailure(message: m.error ?? "The snapshot failed.")
        }
        return .result()
    }
}

struct ListRunningVMsIntent: AppIntent {
    static let title: LocalizedStringResource = "List Running VMs"
    static let description = IntentDescription("Returns the names of the running virtual machines.")
    @MainActor func perform() async throws -> some IntentResult & ReturnsValue<[String]> {
        let m = try readySession()
        return .result(value: m.vms.filter(\.isRunning).map(\.name))
    }
}

struct MachinaShortcuts: AppShortcutsProvider {
    static var appShortcuts: [AppShortcut] {
        AppShortcut(intent: ListRunningVMsIntent(), phrases: ["List running VMs in \(.applicationName)"], shortTitle: "Running VMs", systemImageName: "server.rack")
        AppShortcut(intent: StartVMIntent(), phrases: ["Start \(\.$vm) in \(.applicationName)"], shortTitle: "Start VM", systemImageName: "play.fill")
        AppShortcut(intent: ShutDownVMIntent(), phrases: ["Shut down \(\.$vm) in \(.applicationName)"], shortTitle: "Shut Down VM", systemImageName: "power")
    }
}
