import Foundation

// Fleet Cloud launch wizard model. Endpoints confirmed against web/src/api/{flavors,nativeTemplates,nativeNetworks,nativeKeypairs,nativeVms}.ts
// and controller/src/api/vms/mod.rs (`run_instances`, `CreateFromTemplateBody`, `MAX_RUN_COUNT = 20`):
//   GET  /api/v1/flavors | /templates | /networks | /keypairs        (controller proxy)
//   POST /api/v1/vms/run-instances  {template_ref, name, flavor_id, network?, key_name?, cloud_init_user_data?, count, min_count?}
//        -> {requested, created, instances: [{name, task_id}], error?}; names are `name` for one, `name-1 … name-N` for several.

public struct InstanceFlavor: Decodable, Hashable, Identifiable, Sendable {
    public var id: String
    public var name: String
    public var vcpus: Int
    public var memoryMib: Int
    public var diskGib: Int
    enum CodingKeys: String, CodingKey { case id, name, vcpus, memoryMib = "memory_mib", diskGib = "disk_gib" }
    public var summary: String { "\(vcpus) vCPU · \(memoryMib >= 1024 ? String(format: "%g GiB", Double(memoryMib) / 1024) : "\(memoryMib) MiB") · \(diskGib) GiB disk" }
}

public struct InstanceImage: Decodable, Hashable, Identifiable, Sendable {
    public var id: String
    public var name: String
    public var version: String?
    public var osFamily: String?
    public var description: String?
    enum CodingKeys: String, CodingKey { case id, name, version, description, osFamily = "os_family" }
}

public struct InstanceNetwork: Decodable, Hashable, Identifiable, Sendable {
    public var id: String
    public var name: String
    public var backend: String?
}

public struct InstanceKeypair: Decodable, Hashable, Identifiable, Sendable {
    public var id: String
    public var name: String
}

public struct InstanceCatalog: Sendable {
    public var flavors: [InstanceFlavor] = []
    public var images: [InstanceImage] = []
    public var networks: [InstanceNetwork] = []
    public var keypairs: [InstanceKeypair] = []
    /// Why a catalog could not be read (flavors and images are needed to launch; networks and key pairs are optional).
    public var problems: [String] = []
    public init() {}
}

public struct RunInstancesResult: Decodable, Hashable, Sendable {
    public struct Instance: Decodable, Hashable, Sendable { public var name: String; public var taskId: String
        enum CodingKeys: String, CodingKey { case name, taskId = "task_id" } }
    public var requested: Int
    public var created: Int
    public var instances: [Instance]
    public var error: String?
}

public struct InstanceLaunch: Hashable, Sendable {
    public static let maxCount = 20
    public static let maxUserDataBytes = 16 * 1024
    public struct Invalid: Error, LocalizedError, Sendable { public var message: String; public var errorDescription: String? { message } }

    public var name = ""
    public var imageName = ""        // sent as template_ref (the web UI sends the image's name)
    public var flavorId = ""
    public var network = ""          // network name; empty keeps the server default
    public var keyName = ""          // saved key pair name; empty = none
    public var count = 1
    public var userData = ""         // cloud-init #cloud-config or script
    public init() {}

    /// The instance names the server will create.
    public var plannedNames: [String] {
        let n = name.trimmingCharacters(in: .whitespaces)
        return count <= 1 ? [n] : (1...count).map { "\(n)-\($0)" }
    }

    /// Field-level checks the wizard runs before each step; the server stays the authority.
    public func validateName() throws {
        let n = name.trimmingCharacters(in: .whitespaces)
        if n.isEmpty { throw Invalid(message: "A name is required.") }
        if n.count + (count > 1 ? 3 : 0) > 64 { throw Invalid(message: "The name is too long (64 characters including the -N suffix).") }
        if !(n.first?.isASCII == true && (n.first?.isLetter == true || n.first?.isNumber == true)) { throw Invalid(message: "The name must start with a letter or number.") }
    }

    public func validate() throws {
        try validateName()
        if imageName.isEmpty { throw Invalid(message: "Pick an image.") }
        if flavorId.isEmpty { throw Invalid(message: "Pick a flavor.") }
        if !(1...Self.maxCount).contains(count) { throw Invalid(message: "Count must be between 1 and \(Self.maxCount).") }
        if userData.utf8.count > Self.maxUserDataBytes { throw Invalid(message: "User data is \(userData.utf8.count) bytes; the limit is \(Self.maxUserDataBytes).") }
    }

    public func body() throws -> JSONValue {
        try validate()
        var o: [String: JSONValue] = [
            "name": .string(name.trimmingCharacters(in: .whitespaces)), "template_ref": .string(imageName),
            "flavor_id": .string(flavorId), "count": .number(Double(count)),
        ]
        if !network.isEmpty { o["network"] = .string(network) }
        if !keyName.isEmpty { o["key_name"] = .string(keyName) }
        if !userData.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty { o["cloud_init_user_data"] = .string(userData) }
        return .object(o)
    }

    public func endpoint() throws -> Endpoint { Endpoint("POST", "/api/v1/vms/run-instances", body: try body(), controller: true) }
}

public extension MachinaClient {
    func instanceCatalog() async throws -> InstanceCatalog {
        async let f = capture { try await self.list("/api/v1/flavors", as: InstanceFlavor.self) }
        async let i = capture { try await self.list("/api/v1/templates", as: InstanceImage.self) }
        async let n = capture { try await self.list("/api/v1/networks", as: InstanceNetwork.self) }
        async let k = capture { try await self.list("/api/v1/keypairs", as: InstanceKeypair.self) }
        var c = InstanceCatalog()
        switch await f { case .success(let v): c.flavors = v; case .failure(let e): c.problems.append("flavors: \(e.localizedDescription)") }
        switch await i { case .success(let v): c.images = v; case .failure(let e): c.problems.append("images: \(e.localizedDescription)") }
        switch await n { case .success(let v): c.networks = v; case .failure(let e): c.problems.append("networks: \(e.localizedDescription)") }
        switch await k { case .success(let v): c.keypairs = v; case .failure(let e): c.problems.append("key pairs: \(e.localizedDescription)") }
        return c
    }

    private func list<T: Decodable & Sendable>(_ path: String, as: T.Type) async throws -> [T] {
        try JSONDecoder().decode([T].self, from: try await send("GET", Endpoint.controllerGet(path).resolvedPath))
    }

    private func capture<T: Sendable>(_ body: @Sendable () async throws -> T) async -> Result<T, Error> {
        do { return .success(try await body()) } catch { return .failure(error) }
    }

    func runInstances(_ launch: InstanceLaunch) async throws -> RunInstancesResult {
        let e = try launch.endpoint()
        let data = try await send(e.method, e.resolvedPath, body: try JSONEncoder().encode(e.body))
        return try JSONDecoder().decode(RunInstancesResult.self, from: data)
    }
}
