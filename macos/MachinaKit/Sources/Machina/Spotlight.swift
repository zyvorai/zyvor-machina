import Foundation
import CoreSpotlight
import UniformTypeIdentifiers
import MachinaKit

/// Makes VMs findable from Spotlight; choosing one opens it in the app. Rebuilt at most every few seconds, and
/// cleared on sign-out so another server's VMs never linger in the index.
@MainActor
final class SpotlightIndexer {
    static let shared = SpotlightIndexer()
    static let activityType = CSSearchableItemActionType
    private static let domain = "dev.zyvor.machina.vm"
    private var pending: Task<Void, Never>?

    func schedule(_ vms: [VmInfo]) {
        pending?.cancel()
        pending = Task { [vms] in
            try? await Task.sleep(nanoseconds: 3_000_000_000)
            guard !Task.isCancelled else { return }
            let items = vms.map { vm -> CSSearchableItem in
                let a = CSSearchableItemAttributeSet(contentType: .item)
                a.title = vm.name
                a.contentDescription = "\(vm.state.capitalized) · \(vm.vcpus) vCPU · \(vm.memoryMb) MiB" + (vm.guestIp.map { " · \($0)" } ?? "")
                a.keywords = ["vm", "machina", "virtual machine", vm.state]
                return CSSearchableItem(uniqueIdentifier: "vm:" + vm.name, domainIdentifier: Self.domain, attributeSet: a)
            }
            let index = CSSearchableIndex.default()
            try? await index.deleteSearchableItems(withDomainIdentifiers: [Self.domain])
            if !items.isEmpty { try? await index.indexSearchableItems(items) }
        }
    }

    func clear() {
        pending?.cancel()
        CSSearchableIndex.default().deleteSearchableItems(withDomainIdentifiers: [Self.domain]) { _ in }
    }

    nonisolated static func vmName(from activity: NSUserActivity) -> String? {
        guard let id = activity.userInfo?[CSSearchableItemActivityIdentifier] as? String, id.hasPrefix("vm:") else { return nil }
        return String(id.dropFirst(3))
    }
}
