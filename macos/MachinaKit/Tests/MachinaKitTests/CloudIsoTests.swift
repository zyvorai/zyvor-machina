import Testing
import Foundation
@testable import MachinaKit

private func makeTestClient() -> MachinaClient {
    let cfg = URLSessionConfiguration.ephemeral
    cfg.protocolClasses = [MockProtocol.self]
    return MachinaClient(server: ServerProfile(name: "t", url: URL(string: "https://h:5092")!), session: URLSession(configuration: cfg))
}

private func bodyData(_ req: URLRequest) -> Data? {
    req.httpBody ?? req.httpBodyStream.map { st in
        st.open(); defer { st.close() }
        var d = Data(); var buf = [UInt8](repeating: 0, count: 4096)
        while st.hasBytesAvailable { let n = st.read(&buf, maxLength: buf.count); if n <= 0 { break }; d.append(buf, count: n) }
        return d
    }
}

private func record(_ seen: Counter, _ req: URLRequest) {
    var s = "\(req.httpMethod ?? "") \(req.url!.path)"
    if let q = req.url!.query { s += "?" + q }
    if let b = bodyData(req), !b.isEmpty { s += " " + String(decoding: b, as: UTF8.self) }
    seen.add(s)
}

private func jsonBody(_ s: String) -> JSONValue { try! JSONDecoder().decode(JSONValue.self, from: Data(s.utf8)) }

private func tempISO(_ name: String, bytes: Int = 2048) throws -> URL {
    let dir = FileManager.default.temporaryDirectory.appendingPathComponent("machina-iso-\(UUID().uuidString)")
    try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    let f = dir.appendingPathComponent(name)
    try Data(repeating: 0x41, count: bytes).write(to: f)
    return f
}

// ISO upload / download

extension ClientTests {
    @Test func isoFilenameAndUrlRules() throws {
        for ok in ["ubuntu.iso", "Win11_24H2.ISO", "a b.iso"] { try IsoRules.validate(filename: ok) }
        for bad in ["", "x.img", ".iso", ".hidden.iso", "-x.iso", "a/b.iso", "a\\b.iso", "noext", String(repeating: "a", count: 260) + ".iso"] {
            #expect(throws: IsoRules.Invalid.self) { try IsoRules.validate(filename: bad) }
        }
        #expect(try IsoRules.validate(downloadURL: " https://example.com/a.iso ").host == "example.com")
        for bad in ["ftp://x/a.iso", "file:///etc/passwd", "example.com/a.iso", "https://", ""] {
            #expect(throws: IsoRules.Invalid.self) { try IsoRules.validate(downloadURL: bad) }
        }
        // Dropped items: folders and non-ISO files are skipped.
        let iso = try tempISO("boot.iso"), txt = try tempISO("notes.txt")
        let r = IsoRules.split([iso, txt, iso.deletingLastPathComponent(), URL(string: "https://x/y.iso")!])
        #expect(r.accepted == [iso] && r.skipped.count == 3)
    }

    @Test func isoListDecodes() async throws {
        MockProtocol.handler = { _ in (200, #"{"files":[{"path":"/var/lib/libvirt/images/a.iso","name":"a.iso","size_bytes":1234,"format":"iso"}],"scan_directories":["/x"]}"#) }
        let isos = try await makeTestClient().listIsos()
        #expect(isos.count == 1 && isos[0].sizeBytes == 1234 && isos[0].id == "/var/lib/libvirt/images/a.iso")
    }

    @Test func uploadIsoRequestShapeProgressAndErrors() async throws {
        let seen = Counter()
        let ctype = Counter()
        MockProtocol.handler = { req in
            record(seen, req); ctype.add(req.value(forHTTPHeaderField: "Content-Type") ?? "")
            return (200, #"{"status":"ok","name":"my disk.iso","path":"/var/lib/machina/isos/my disk.iso","size_bytes":2048}"#)
        }
        let f = try tempISO("my disk.iso")
        let last = Counter()
        let res = try await makeTestClient().uploadIso(file: f, overwrite: true) { sent, total in last.add("\(sent)/\(total)") }
        #expect(res.sizeBytes == 2048 && res.name == "my disk.iso")
        let calls = seen.all()
        #expect(calls.count == 1)
        #expect(calls[0].hasPrefix("POST /api/v1/browse/isos/upload?filename=my%20disk.iso&overwrite=true"))   // bare name, query only, no job
        #expect(ctype.all() == ["application/octet-stream"])
        #expect(last.all().last == "2048/2048")

        // Without overwrite the flag is absent.
        _ = try await makeTestClient().uploadIso(file: f)
        #expect(seen.all()[1].contains("overwrite") == false)

        // A server error carries the daemon's message.
        MockProtocol.handler = { _ in (400, #"{"error":"my disk.iso already exists — not overwriting"}"#) }
        do { _ = try await makeTestClient().uploadIso(file: f); Issue.record("expected an error") }
        catch { #expect(error.localizedDescription.contains("already exists")) }

        // Rejected before any request: not an .iso, empty file.
        MockProtocol.handler = { req in record(seen, req); return (200, "{}") }
        let n = seen.all().count
        let txt = try tempISO("a.txt"), empty = try tempISO("e.iso", bytes: 0)
        await #expect(throws: IsoRules.Invalid.self) { try await makeTestClient().uploadIso(file: txt) }
        await #expect(throws: IsoRules.Invalid.self) { try await makeTestClient().uploadIso(file: empty) }
        #expect(seen.all().count == n)
    }

    @Test func downloadStartsAJobAndPollsToCompletion() async throws {
        let seen = Counter()
        let polls = Counter()
        MockProtocol.handler = { req in
            record(seen, req)
            if req.httpMethod == "POST" { return (200, #"{"status":"started","job_id":"11111111-2222-3333-4444-555555555555","name":"debian.iso","path":"/var/lib/machina/isos/debian.iso"}"#) }
            polls.add("p")
            switch polls.count("p") {
            case 1: return (200, #"{"id":"j","kind":"iso_download","title":"t","status":"running","bytes_done":50,"bytes_total":200,"created_unix":1,"updated_unix":1}"#)
            case 2: return (200, #"{"id":"j","status":"running","bytes_done":150,"created_unix":1,"updated_unix":1,"logs":[]}"#)   // size unknown
            default: return (200, #"{"id":"j","status":"completed","bytes_done":200,"bytes_total":200,"created_unix":1,"updated_unix":2}"#)
            }
        }
        let c = makeTestClient()
        let started = try await c.startIsoDownload(url: "https://mirror.example/debian.iso", filename: "debian.iso", overwrite: true)
        #expect(started.jobId == "11111111-2222-3333-4444-555555555555")
        let post = seen.all()[0]
        #expect(post.hasPrefix("POST /api/v1/browse/isos/download "))
        let body = jsonBody(String(post.dropFirst("POST /api/v1/browse/isos/download ".count)))
        #expect(body["url"]?.text == "https://mirror.example/debian.iso" && body["filename"]?.text == "debian.iso" && body["overwrite"]?.text == "yes")

        let fractions = Counter()
        let final = try await c.waitForJob(started.jobId, interval: .milliseconds(5)) { j in fractions.add(j.fraction.map { String(format: "%.2f", $0) } ?? "nil") }
        #expect(final.status == "completed")
        #expect(fractions.all() == ["0.25", "nil", "1.00"])           // unknown total = indeterminate, never a made-up percentage
        #expect(seen.all().contains("GET /api/v1/jobs/11111111-2222-3333-4444-555555555555"))

        MockProtocol.handler = { _ in (200, #"{"id":"j","status":"failed","error":"HTTP 404 from the mirror","created_unix":1,"updated_unix":1}"#) }
        do { _ = try await c.waitForJob("j", interval: .milliseconds(5)); Issue.record("expected failure") }
        catch { #expect(error.localizedDescription == "HTTP 404 from the mirror") }
    }

    @Test func downloadValidatesBeforeSending() async {
        let seen = Counter()
        MockProtocol.handler = { req in record(seen, req); return (200, "{}") }
        await #expect(throws: IsoRules.Invalid.self) { _ = try await makeTestClient().startIsoDownload(url: "file:///etc/passwd") }
        await #expect(throws: IsoRules.Invalid.self) { _ = try await makeTestClient().startIsoDownload(url: "https://x/a.iso", filename: "a.txt") }
        #expect(seen.all().isEmpty)
    }
}

// Launch wizard

extension ClientTests {
    @Test func launchBodyAndValidation() throws {
        var l = InstanceLaunch()
        #expect(throws: InstanceLaunch.Invalid.self) { try l.validate() }                 // nothing chosen
        l.name = "web"; l.imageName = "ubuntu-24.04"; l.flavorId = "f-1"
        let minimal = try l.body()
        #expect(minimal["template_ref"]?.text == "ubuntu-24.04" && minimal["flavor_id"]?.text == "f-1" && minimal["count"]?.double == 1)
        #expect(minimal["network"] == nil && minimal["key_name"] == nil && minimal["cloud_init_user_data"] == nil)   // empty optionals are left out
        l.count = 3; l.network = "default"; l.keyName = "kp"; l.userData = "#cloud-config\nruncmd: [id]\n"
        let full = try l.body()
        #expect(full["count"]?.double == 3 && full["network"]?.text == "default" && full["key_name"]?.text == "kp" && full["cloud_init_user_data"]?.text.hasPrefix("#cloud-config") == true)
        #expect(l.plannedNames == ["web-1", "web-2", "web-3"])
        let e = try l.endpoint()
        #expect(e.method == "POST" && e.resolvedPath == "/api/v1/platform/controller/api/v1/vms/run-instances")
        l.count = 21; #expect(throws: InstanceLaunch.Invalid.self) { try l.validate() }
        l.count = 0; #expect(throws: InstanceLaunch.Invalid.self) { try l.validate() }
        l.count = 1; l.userData = String(repeating: "x", count: InstanceLaunch.maxUserDataBytes + 1)
        #expect(throws: InstanceLaunch.Invalid.self) { try l.validate() }
        l.userData = ""; l.name = "-bad"; #expect(throws: InstanceLaunch.Invalid.self) { try l.validate() }
        l.name = String(repeating: "a", count: 63); l.count = 2; #expect(throws: InstanceLaunch.Invalid.self) { try l.validate() }   // "-N" suffix would pass 64
    }

    @Test func catalogAndRunInstancesCalls() async throws {
        let seen = Counter()
        MockProtocol.handler = { req in
            record(seen, req)
            switch req.url!.path {
            case "/api/v1/platform/controller/api/v1/flavors": return (200, #"[{"id":"f-1","name":"small","vcpus":2,"memory_mib":2048,"disk_gib":20,"description":"","is_public":true}]"#)
            case "/api/v1/platform/controller/api/v1/templates": return (200, #"[{"id":"t-1","name":"alma-10","version":"1.0.0","os_family":"linux","description":"d"}]"#)
            case "/api/v1/platform/controller/api/v1/networks": return (200, #"[{"id":"n-1","name":"default","backend":"linux-bridge","vlan_id":null}]"#)
            case "/api/v1/platform/controller/api/v1/keypairs": return (500, "boom")
            default: return (200, #"{"requested":2,"created":2,"instances":[{"name":"a-1","task_id":"t1"},{"name":"a-2","task_id":"t2"}],"error":null}"#)
            }
        }
        let c = makeTestClient()
        let cat = try await c.instanceCatalog()
        #expect(cat.flavors.first?.summary == "2 vCPU · 2 GiB · 20 GiB disk" && cat.images.first?.name == "alma-10" && cat.networks.count == 1)
        #expect(cat.keypairs.isEmpty && cat.problems.count == 1 && cat.problems[0].hasPrefix("key pairs"))   // an optional list failing does not block the wizard
        var l = InstanceLaunch(); l.name = "a"; l.imageName = "alma-10"; l.flavorId = "f-1"; l.count = 2
        let r = try await c.runInstances(l)
        #expect(r.created == 2 && r.instances.map(\.name) == ["a-1", "a-2"] && r.error == nil)
        let post = seen.all().last!
        #expect(post.hasPrefix("POST /api/v1/platform/controller/api/v1/vms/run-instances "))
        let body = jsonBody(String(post.dropFirst("POST /api/v1/platform/controller/api/v1/vms/run-instances ".count)))
        #expect(body["name"]?.text == "a" && body["count"]?.double == 2 && body["template_ref"]?.text == "alma-10")
    }
}

// Action inputs, stacks, load balancers, subnets

extension ClientTests {
    private func row(_ json: String) -> ResourceRow { ResourceRow(id: "r", values: jsonBody(json)) }

    @Test func actionInputMergesTypedFieldsIntoTheBody() throws {
        let add = FeatureRegistry.spec("fc-lbs")!.actions.first { $0.title == "Add member…" }!
        let e = try add.endpoint(for: row(#"{"id":"lb/1","name":"x"}"#), input: ["vm_id": "vm-9", "port": "8080", "weight": ""])!
        #expect(e.method == "POST" && e.resolvedPath == "/api/v1/platform/controller/api/v1/load-balancers/lb%2F1/members")
        #expect(e.body?["vm_id"]?.text == "vm-9" && e.body?["port"]?.double == 8080 && e.body?["weight"] == nil)       // empty optional left out
        #expect(throws: ResourceCreate.Invalid.self) { try add.endpoint(for: row(#"{"id":"1"}"#), input: ["vm_id": "v", "port": "http"]) }
        #expect(throws: ResourceCreate.Invalid.self) { try add.endpoint(for: row(#"{"id":"1"}"#), input: ["port": "80"]) }   // vm_id required

        let hc = FeatureRegistry.spec("fc-lbs")!.actions.first { $0.title == "Health check…" }!
        let h = try hc.endpoint(for: row(#"{"id":"1"}"#), input: ["protocol": "http", "path": "/up", "interval_secs": "5"])!
        #expect(h.method == "PUT" && h.path == "/api/v1/load-balancers/1/health-check" && h.body?["protocol"]?.text == "http" && h.body?["interval_secs"]?.double == 5)
        #expect(throws: ResourceCreate.Invalid.self) { try hc.endpoint(for: row(#"{"id":"1"}"#), input: ["protocol": "icmp"]) }

        // A row missing the path value never produces a request.
        #expect(try add.endpoint(for: row(#"{"name":"x"}"#), input: ["vm_id": "v", "port": "1"]) == nil)
    }

    @Test func lbCreateAndMemberPatches() throws {
        let c = FeatureRegistry.spec("fc-lbs")!.create!
        let b = try c.body(from: ["name": "web", "host_id": "h1", "listener_port": "8080", "protocol": "tcp"])
        #expect(b["listener_port"]?.double == 8080 && b["protocol"]?.text == "tcp" && c.endpoint.path == "/api/v1/load-balancers")
        #expect(throws: ResourceCreate.Invalid.self) { try c.body(from: ["name": "web", "host_id": "h1", "listener_port": "80", "protocol": "sctp"]) }
        let m = row(#"{"id":"m1","load_balancer_id":"lb1","weight":3,"enabled":true}"#)
        let specs = FeatureRegistry.spec("fc-lb-members")!
        let disable = try specs.actions.first { $0.title == "Disable" }!.endpoint(for: m, input: [:])!
        #expect(disable.method == "PATCH" && disable.path == "/api/v1/load-balancers/lb1/members/m1" && disable.body?["enabled"]?.text == "no")
        let w = specs.actions.first { $0.title == "Set weight…" }!
        #expect(w.initialValues(for: m) == ["weight": "3"])                                              // pre-filled from the row
        #expect(try w.endpoint(for: m, input: ["weight": "7"])!.body?["weight"]?.double == 7)
        let rm = try specs.actions.first { $0.title == "Remove" }!.endpoint(for: m, input: [:])!
        #expect(rm.method == "DELETE" && rm.body == nil)
    }

    @Test func stackEditSendsAParsedTemplate() throws {
        let spec = FeatureRegistry.spec("fc-stacks")!
        let stack = row(#"{"id":"s1","name":"app","template_json":{"vms":[{"name":"a","memory":"2Gi"}]}}"#)
        let edit = spec.actions.first { $0.title == "Edit template…" }!
        let initial = edit.initialValues(for: stack)["template"] ?? ""
        #expect(initial.contains("\"vms\"") && initial.contains("\n"))                                    // pretty JSON of the row's template
        let e = try edit.endpoint(for: stack, input: ["template": initial.replacingOccurrences(of: "2Gi", with: "4Gi")])!
        #expect(e.method == "PUT" && e.path == "/api/v1/stacks/s1" && e.body?["template"]?["vms"]?.array?.first?["memory"]?.text == "4Gi")   // an object, not a string
        #expect(e.body?.object?.keys.sorted() == ["template"])
        #expect(throws: ResourceCreate.Invalid.self) { try edit.endpoint(for: stack, input: ["template": "{not json"]) }
        #expect(throws: ResourceCreate.Invalid.self) { try edit.endpoint(for: stack, input: ["template": "[1,2]"]) }   // must be an object

        let plan = spec.actions.first { $0.title == "Preview plan…" }!
        let p = try plan.endpoint(for: stack, input: ["template": initial])!
        #expect(plan.showsResult && p.method == "POST" && p.path == "/api/v1/stacks/plan" && p.body?["name"]?.text == "app" && p.body?["stack_id"]?.text == "s1" && p.body?["template"]?.object != nil)

        let create = spec.create!
        let cb = try create.body(from: ["name": "app2", "template": create.fields[1].initial])
        #expect(cb["name"]?.text == "app2" && cb["template"]?["security_groups"]?.array?.count == 1 && create.endpoint.path == "/api/v1/stacks")
    }

    @Test func subnetsComeFromProjectsThenVpcs() async throws {
        MockProtocol.handler = { req in
            switch req.url!.path {
            case "/api/v1/platform/controller/api/v1/project-registry": return (200, #"[{"id":"p1","name":"default"}]"#)
            case "/api/v1/platform/controller/api/v1/cloud/projects/p1/vpcs": return (200, #"[{"id":"v1","name":"net-a","cidr":"10.1.0.0/16"},{"id":"v2","name":"net-b","cidr":"10.2.0.0/16"}]"#)
            case "/api/v1/platform/controller/api/v1/cloud/vpcs/v1/subnets": return (200, #"[{"id":"s1","name":"a1","cidr":"10.1.1.0/24","status":"ready","last_error":""}]"#)
            default: return (500, "boom")                                                                 // net-b fails: skipped
            }
        }
        let spec = FeatureRegistry.spec("fc-subnets")!
        let rows = spec.rows(from: try await makeTestClient().load(spec))
        #expect(rows.count == 1 && rows[0].values["_parent"]?.text == "net-a" && spec.columns.map { $0.render(rows[0]) }.prefix(4) == ["net-a", "a1", "10.1.1.0/24", "ready"])

        let add = FeatureRegistry.spec("fc-vpcs")!.actions.first { $0.title == "Add subnet…" }!
        let e = try add.endpoint(for: row(#"{"id":"v1"}"#), input: ["name": "a2", "cidr": "10.1.2.0/24"])!
        #expect(e.method == "POST" && e.path == "/api/v1/cloud/vpcs/v1/subnets" && e.body?["cidr"]?.text == "10.1.2.0/24" && e.body?["name"]?.text == "a2")
        let del = try spec.actions.first { $0.title == "Delete" }!.endpoint(for: row(#"{"id":"s1"}"#), input: [:])!
        #expect(del.method == "DELETE" && del.path == "/api/v1/cloud/subnets/s1")
        let retry = spec.actions.first { $0.title == "Retry" }!.endpoint(for: row(#"{"id":"s1"}"#))!
        #expect(retry.method == "POST" && retry.path == "/api/v1/cloud/subnets/s1/retry")
    }

    @Test func accessoriesAndWriteSafety() {
        #expect(FeatureRegistry.spec("isos")?.accessory == .isoTransfer && FeatureRegistry.spec("fc-instances")?.accessory == .instanceWizard)
        // Every page that can write declares it with a non-GET method; input actions are never GET.
        for s in FeatureRegistry.all {
            for a in s.actions { #expect(a.method != "GET", "\(s.id) \(a.title)") }
            for a in s.actions where !a.input.isEmpty { #expect(a.method == "POST" || a.method == "PUT" || a.method == "PATCH") }
        }
        // Destructive removals are flagged so the confirmation says so.
        let lb = FeatureRegistry.spec("fc-lbs")!
        #expect(lb.actions.first { $0.title == "Delete" }!.destructive && FeatureRegistry.spec("fc-lb-members")!.actions.first { $0.title == "Remove" }!.destructive)
    }
}
