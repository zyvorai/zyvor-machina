import Testing
import Foundation
@testable import MachinaKit

private func host(_ id: String, _ name: String, state: String = "online", memTotal: Double? = 10000, memUsed: Double? = 2000) -> PlatformHost {
    PlatformHost(id: id, hostname: name, address: nil, state: state, maintenanceMode: false, schedulable: true, fenced: false,
                 vmCount: nil, cpuPercent: 10, memoryUsedMib: memUsed, memoryTotalMib: memTotal, lastHeartbeatAt: nil)
}
private func vm(_ name: String, _ host: String?, _ state: String = "running", mem: Int = 1000) -> PlatformVM {
    PlatformVM(id: name, name: name, hostId: host, observedState: state, vcpus: 1, memoryMib: mem)
}

@Suite struct InsightTests {
    @Test func toneMapping() {
        #expect(Tone.vm("Running") == .ok)
        #expect(Tone.vm("paused") == .warn)
        #expect(Tone.vm("crashed") == .bad)
        #expect(Tone.vm("shutoff") == .idle)
        #expect(Tone.load(95) == .bad && Tone.load(80) == .warn && Tone.load(10) == .ok && Tone.load(nil) == .idle)
    }

    @Test func platformVMDecodesWithMissingFields() throws {
        let v = try JSONDecoder().decode(PlatformVM.self, from: Data(#"{"id":"1","name":"a","host_id":"h","observed_state":"running","vcpus":2,"memory_mib":512}"#.utf8))
        #expect(v.hostId == "h" && v.memoryMib == 512 && v.tone == .ok)
        let w = try JSONDecoder().decode(PlatformVM.self, from: Data(#"{"id":"2","name":"b"}"#.utf8))
        #expect(w.hostId == nil && w.observedState == "unknown" && w.vcpus == 0)
    }

    @Test func topologyGroupsAndOrders() {
        let hosts = [host("2", "zeta", state: "offline"), host("1", "beta"), host("3", "alpha")]
        let vms = [vm("b", "1", "shutoff"), vm("a", "1"), vm("c", "1", "crashed"), vm("lost", "gone"), vm("nohost", nil)]
        let g = Topology.group(hosts: hosts, vms: vms)
        #expect(g.map(\.name) == ["alpha", "beta", "zeta", "Unassigned"])
        #expect(g[1].vms.map(\.name) == ["c", "a", "b"])   // crashed, running, stopped
        #expect(g[3].vms.map(\.name).sorted() == ["lost", "nohost"])
        #expect(g[1].running == 1 && g[2].tone == .bad)
        #expect(Topology.group(hosts: hosts, vms: []).count == 3)
    }

    @Test func heatLevelsAndHeadroom() {
        #expect(Heat.level(nil) == nil)
        #expect([0.0, 30, 60, 80, 99].map { Heat.level($0)! } == [0, 1, 2, 3, 4])
        // 10000 MiB host, 2000 used, running VMs were given 3000: committed (3000) beats used (2000) -> 7000 free -> 3 x 2 GiB
        let g = HostGroup(host: host("1", "h"), name: "h", vms: [vm("a", "1", mem: 1000), vm("b", "1", mem: 2000), vm("off", "1", "shutoff", mem: 5000)])
        #expect(Heat.memoryCommitPercent(g) == 30)
        #expect(Heat.headroom(g, sizeMib: 2048) == 3)
        let off = HostGroup(host: host("2", "o", state: "offline"), name: "o", vms: [])
        #expect(Heat.headroom(off) == 0)
        #expect(Heat.memoryCommitPercent(HostGroup(host: nil, name: "x", vms: [])) == nil)
    }

    @Test func bucketing() {
        #expect(Heat.bucketed([1, 2, 3], columns: 5) == [1, 2, 3])
        #expect(Heat.bucketed([1, 3, 5, 7], columns: 2) == [2, 6])
        #expect(Heat.bucketed([], columns: 3).isEmpty && Heat.bucketed([1], columns: 0).isEmpty)
    }

    @Test func sparklineAndOrdering() {
        func pt(_ t: Double, _ cpuNs: Double, _ pctMem: Double) -> MetricsPoint {
            MetricsPoint(timestampMs: t * 1000, vmMetrics: [VmMetricsSample(name: "a", cpuTimeNs: cpuNs, vcpus: 1, memoryPct: pctMem)])
        }
        // 1 core: 0.5 s of CPU per second = 50%
        let pts = [pt(0, 0, 10), pt(1, 5e8, 20), pt(2, 1e9, 30)]
        #expect(Spark.cpu(pts, vm: "a") == [50, 50])
        #expect(Spark.cpu(pts, vm: "a", count: 1).count == 1)
        #expect(Spark.memoryPercent(pts, vm: "a") == [10, 20, 30])
        #expect(Spark.cpu(pts, vm: "missing").isEmpty)

        let vms = [VmInfo(name: "vm10", state: "shutoff", vcpus: 1, memoryMb: 1), VmInfo(name: "vm2", state: "running", vcpus: 1, memoryMb: 1),
                   VmInfo(name: "vm3", state: "crashed", vcpus: 1, memoryMb: 1)]
        #expect(Spark.order(vms, by: .name, cpu: [:]).map(\.name) == ["vm2", "vm3", "vm10"])
        #expect(Spark.order(vms, by: .state, cpu: [:]).map(\.name) == ["vm3", "vm2", "vm10"])
        #expect(Spark.order(vms, by: .cpu, cpu: ["vm2": 5, "vm10": 40]).map(\.name) == ["vm10", "vm2", "vm3"])
    }

    @Test func activityLogAppendCapAndBuckets() {
        let t0 = Date(timeIntervalSince1970: 1_000_000)
        var log = ActivityLog.append([], [VmChange(event: "state_change", name: "a", oldState: "running", newState: "shutoff"),
                                          VmChange(event: "state_change", name: "b", oldState: "shutoff", newState: "running")], at: t0)
        #expect(log.map(\.vm) == ["b", "a"])           // newest first within a batch too
        #expect(log[0].summary == "shutoff \u{2192} running" && log[0].tone == .ok)
        log = ActivityLog.append(log, [VmChange(event: "state_change", name: "c", oldState: nil, newState: "crashed")], at: t0.addingTimeInterval(400))
        #expect(log.first?.vm == "c" && log.first?.tone == .bad)
        #expect(ActivityLog.append(log, [VmChange(event: "added", name: "d", oldState: nil, newState: nil)], at: t0, cap: 2).count == 2)

        let b = ActivityLog.buckets(log, slot: 300, slots: 12, now: t0.addingTimeInterval(500))
        #expect(b.reduce(0) { $0 + $1.count } == 3)
        #expect(ActivityLog.buckets(log, slot: 300, slots: 12, now: t0.addingTimeInterval(10_000)).isEmpty)  // all too old
        #expect(ActivityLog.buckets(log, slot: 0, slots: 12, now: t0).isEmpty)
        #expect(ActivityLog.filter(log, vm: "B").map(\.vm) == ["b"] && ActivityLog.filter(log, vm: "").count == 3)
    }
}
