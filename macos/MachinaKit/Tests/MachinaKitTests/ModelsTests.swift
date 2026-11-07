import Testing
import Foundation
@testable import MachinaKit

@Test func decodesVmInfo() throws {
    let json = #"[{"name":"web1","state":"running","vcpus":2,"memory_mb":2048,"guest_ip":"10.0.0.5","libvirt_connection":"session"}]"#
    let vms = try JSONDecoder().decode([VmInfo].self, from: Data(json.utf8))
    #expect(vms[0].isRunning)
    #expect(vms[0].memoryMb == 2048)
    #expect(vms[0].scopeQuery == [URLQueryItem(name: "connection", value: "session")])
}

@Test func fluxvmScope() {
    let vm = VmInfo(name: "f", state: "stopped", vcpus: 1, memoryMb: 512, libvirtConnection: "fluxvm")
    #expect(vm.scopeQuery.first?.name == "backend")
}
