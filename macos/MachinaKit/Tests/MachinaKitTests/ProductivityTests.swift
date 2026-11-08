import Testing
import Foundation
@testable import MachinaKit

private func vm(_ n: String, _ s: String = "running", cpu: Int = 2, mem: Int = 2048, ip: String? = nil) -> VmInfo {
    VmInfo(name: n, state: s, vcpus: cpu, memoryMb: mem, guestIp: ip)
}

@Test func fuzzySubsequenceAndRanking() {
    #expect(Fuzzy.score("pgp", in: "postgres-primary") != nil)
    #expect(Fuzzy.score("xyz", in: "postgres") == nil)
    #expect(Fuzzy.score("", in: "x") == 0)
    let ranked = Fuzzy.rank(["alert-gw", "gateway", "api-gateway-01"], query: "gate") { [$0] }
    #expect(!ranked.contains("alert-gw"))
    #expect(ranked.first == "gateway")
    let items = ["Fleet", "Firewall targets", "Storage pools"]
    #expect(Fuzzy.rank(items, query: "sp") { [$0] }.first == "Storage pools")
}

@Test func recentsDedupAndCap() {
    var r: [String] = []
    for i in 0..<12 { r = Recents.pushing("x\(i)", onto: r) }
    r = Recents.pushing("x5", onto: r)
    #expect(r.count == 8 && r.first == "x5" && Set(r).count == 8)
    let out = Recents.promoting(["a", "b", "c", "d"], recents: ["c", "a"]) { $0 }
    #expect(out == ["c", "a", "b", "d"])
}

@Test func pinsToggleAndSplit() {
    var p = Pins.toggling("b", in: [])
    p = Pins.toggling("a", in: p)
    #expect(p == ["b", "a"])
    let s = Pins.split([vm("a"), vm("b"), vm("c")], pins: ["b", "gone", "a"])
    #expect(s.pinned.map(\.name) == ["b", "a"] && s.others.map(\.name) == ["c"])
    #expect(Pins.toggling("a", in: p) == ["b"])
}

@Test func vmFilterQueryLanguage() {
    let vms = [vm("web-01", ip: "10.0.0.5"), vm("web-02", "stopped", cpu: 8), vm("db", mem: 16384, ip: "10.1.0.9")]
    #expect(VMFilter("is:running").apply(to: vms).map(\.name) == ["web-01", "db"])
    #expect(VMFilter("web vcpus>=4").apply(to: vms).map(\.name) == ["web-02"])
    #expect(VMFilter("mem>=4096").apply(to: vms).map(\.name) == ["db"])
    #expect(VMFilter("ip:10.1").apply(to: vms).map(\.name) == ["db"])
    #expect(VMFilter("  ").isEmpty)
    #expect(VMFilter("is:running web").apply(to: vms).map(\.name) == ["web-01"])
}

@Test func savedFiltersReplaceByName() {
    var l = SavedFilter.saving(SavedFilter(name: "Big", query: "mem>=8192"), into: [])
    l = SavedFilter.saving(SavedFilter(name: "Off", query: "is:stopped"), into: l)
    l = SavedFilter.saving(SavedFilter(name: "Big", query: "mem>=4096"), into: l)
    #expect(l.map(\.name) == ["Big", "Off"] && l[0].query == "mem>=4096")
    #expect(SavedFilter.saving(SavedFilter(name: " ", query: "x"), into: l) == l)
}

@Test func bulkPlanSkipsIneligible() {
    let vms = [vm("a"), vm("b", "stopped"), vm("c", "paused")]
    #expect(BulkAction.start.plan(vms).apply.map(\.name) == ["b"])
    #expect(BulkAction.shutdown.plan(vms).skip.map(\.name) == ["b", "c"])
    #expect(BulkAction.resume.plan(vms).apply.map(\.name) == ["c"])
    #expect(BulkAction.snapshot.plan(vms).skip.isEmpty)
    #expect(BulkAction.forceStop.destructive && !BulkAction.reboot.destructive)
    #expect(BulkAction.forceStop.power == .stop && BulkAction.snapshot.power == nil)
    #expect(BulkAction.snapshotName(at: Date(timeIntervalSince1970: 0)) == "bulk-19700101-000000")
}

@Test func csvEscapingAndFormulaGuard() {
    let cols = [ResourceColumn("Name", "name"), ResourceColumn("Note", "note")]
    let rows = [ResourceRow(id: "1", values: .object(["name": .string("a,b"), "note": .string("say \"hi\"")])),
                ResourceRow(id: "2", values: .object(["name": .string("=SUM(A1)")]))]
    let csv = ResourceExport.csv(columns: cols, rows: rows)
    #expect(csv == "Name,Note\r\n\"a,b\",\"say \"\"hi\"\"\"\r\n'=SUM(A1),\r\n")
    #expect(ResourceExport.csvField("-5") == "-5")
    #expect(ResourceExport.fileName("Storage pools!", ext: "csv") == "storage-pools.csv")
    #expect(ResourceExport.json(rows: rows).contains("\"name\" : \"a,b\""))
}

@Test func curlCommands() {
    let server = URL(string: "https://h:5092")!
    let e = Endpoint("POST", "/api/v1/vms/my%20vm/start", query: [URLQueryItem(name: "backend", value: "fluxvm")])
    #expect(Curl.command(e, server: server) == "curl -sk -b $MACHINA_JAR -X POST 'https://h:5092/api/v1/vms/my%20vm/start?backend=fluxvm'")
    let c = Endpoint("POST", "/x", body: .object(["n": .string("it's")]), controller: true)
    let cmd = Curl.command(c, server: server)
    #expect(cmd.contains("/api/v1/platform/controller/x") && cmd.contains("-d '{\"n\":\"it'\\''s\"}'"))
    #expect(Curl.script(e, server: server).contains("$MACHINA_PASS"))
}

@Test func shortcutCatalogHasNoDuplicateKeys() {
    let keys = ShortcutInfo.all.map(\.keys)
    #expect(Set(keys).count == keys.count)
}
