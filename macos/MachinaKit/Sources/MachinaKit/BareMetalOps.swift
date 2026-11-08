import Foundation

/// Never lets a secret leave the app in text form (copied curl, exports, logs).
public enum SecretRedaction {
    public static let placeholder = "<redacted>"
    static let markers = ["password", "passwd", "secret", "token", "api_key", "apikey"]

    public static func isSecretKey(_ key: String) -> Bool {
        let k = key.lowercased()
        return markers.contains { k.contains($0) }
    }

    /// The value with every secret-looking key replaced by `<redacted>` (recursively).
    public static func redact(_ v: JSONValue) -> JSONValue {
        switch v {
        case .object(let o):
            return .object(o.reduce(into: [:]) { out, kv in
                out[kv.key] = isSecretKey(kv.key) && kv.value != .null ? .string(placeholder) : redact(kv.value)
            })
        case .array(let a): return .array(a.map(redact))
        default: return v
        }
    }

    /// True when the body carries a secret (so it must only travel over TLS).
    public static func carriesSecret(_ v: JSONValue?) -> Bool {
        guard let v else { return false }
        return redact(v) != v
    }

    /// Refuses to send a body with a secret over plain http.
    public static func requireTLS(body: JSONValue?, server: URL) throws {
        if carriesSecret(body), server.scheme?.lowercased() != "https" { throw BareMetalError.plainHTTP }
    }
}

public enum BareMetalError: Error, LocalizedError, Sendable, Equatable {
    case plainHTTP, notAdmin, badSolPath, notIPMI, noBmcLogin
    public var errorDescription: String? {
        switch self {
        case .plainHTTP: return "Refusing to send a BMC password over plain http. Connect with https (a pinned TLS session)."
        case .notAdmin: return "Serial-over-LAN needs the admin role."
        case .badSolPath: return "The controller returned an unexpected console path; nothing was opened."
        case .notIPMI: return "Serial-over-LAN needs an IPMI BMC (bmc_type ipmi)."
        case .noBmcLogin: return "This server has no BMC login yet. Use Set BMC login first."
        }
    }
}

/// Per-field input checks that run before anything is sent (shared by every bare-metal form).
public enum FieldCheck: Hashable, Sendable {
    case bmcAddress, bmcUsername, bmcPassword, mediaURL, osName, timezone

    /// nil when the value is acceptable, else what is wrong.
    public func problem(_ s: String) -> String? {
        switch self {
        case .bmcAddress: return BareMetalOps.addressProblem(s)
        case .bmcUsername: return BareMetalOps.usernameProblem(s)
        case .bmcPassword: return BareMetalOps.passwordProblem(s)
        case .mediaURL: return BareMetalOps.mediaURLProblem(s)
        case .osName: return BareMetalOps.osProblem(s)
        case .timezone: return BareMetalOps.timezoneProblem(s)
        }
    }
}

/// Bare-metal write features: BMC credentials, image deploy, virtual media and serial-over-LAN.
/// Routes (controller `api/baremetal.rs`, all admin-only): `PUT servers/{id}/bmc`, `POST servers/{id}/deploy`,
/// `POST servers/{id}/virtual-media`, `POST servers/{id}/sol-token` then WebSocket `/ws/v1/baremetal/{id}/sol`.
public enum BareMetalOps {
    public static let bmcTypes = ["redfish", "ipmi"]
    public static let bootMethods = ["pxe", "virtual_media", "virtual_media_uefi"]
    /// Built-in installers; image-catalog names are accepted too (the controller validates them).
    public static let stockOS = ["ubuntu-22.04", "ubuntu-24.04", "rhel-9", "rocky-9", "almalinux-9"]
    static let serverPath = "/api/v1/baremetal/servers/{id}"

    // MARK: Validation

    private static func hasControl(_ s: String) -> Bool { s.unicodeScalars.contains { $0.value < 0x20 || $0.value == 0x7f } }

    public static func addressProblem(_ s: String) -> String? {
        if s.isEmpty || s.count > 255 { return "must be 1 to 255 characters" }
        if hasControl(s) || s.contains(where: { " '\"`$;&|<>\\()*".contains($0) }) { return "contains characters that are not allowed" }
        if s.contains("@") { return "must not contain a login (use the user name field)" }
        if let r = s.range(of: "://") {
            let scheme = s[..<r.lowerBound].lowercased()
            if scheme != "https" && scheme != "http" { return "URL scheme must be https or http" }
        }
        return nil
    }

    public static func usernameProblem(_ s: String) -> String? {
        if s.isEmpty || s.count > 64 { return "must be 1 to 64 characters" }
        if hasControl(s) || s.contains(where: { " '\"`$;&|<>\\/:".contains($0) }) { return "contains characters that are not allowed" }
        return nil
    }

    public static func passwordProblem(_ s: String) -> String? {
        if s.isEmpty { return "is required" }
        if s.count > 128 { return "is too long (128 characters at most)" }
        if hasControl(s) { return "must not contain control characters or line breaks" }
        return nil
    }

    /// Virtual media is fetched by the BMC itself: plain http(s), a host, and no credentials in the URL.
    public static func mediaURLProblem(_ s: String) -> String? {
        guard let u = URL(string: s), let scheme = u.scheme?.lowercased(), scheme == "https" || scheme == "http" else {
            return "must be an http or https URL"
        }
        if (u.host ?? "").isEmpty { return "needs a host name" }
        if u.user != nil || u.password != nil { return "must not carry a login (user:pass@)" }
        if hasControl(s) || s.contains(" ") { return "contains characters that are not allowed" }
        return nil
    }

    public static func osProblem(_ s: String) -> String? {
        let ok = !s.isEmpty && s.count <= 64 && s.allSatisfy { $0.isASCII && ($0.isLetter || $0.isNumber || "-._".contains($0)) }
        return ok ? nil : "use letters, digits, - . _ (an installer such as ubuntu-24.04 or an image name)"
    }

    public static func timezoneProblem(_ s: String) -> String? {
        let ok = !s.isEmpty && s.count <= 64 && !s.contains("..") && s.allSatisfy { $0.isASCII && ($0.isLetter || $0.isNumber || "/_+-".contains($0)) }
        return ok ? nil : "must be a time zone name such as UTC or Europe/Berlin"
    }

    // MARK: Forms

    private static func bmcFields(passwordRequired: Bool) -> [ResourceCreate.Field] {
        [.init("bmc_type", "BMC type", initial: "redfish", options: bmcTypes),
         .init("bmc_address", "BMC address (host, host:port or URL)", check: .bmcAddress),
         .init("bmc_username", "BMC user name", check: .bmcUsername),
         .init("bmc_password", passwordRequired ? "New BMC password" : "BMC password (leave empty to keep the stored one)",
               required: passwordRequired, secret: true, check: .bmcPassword),
         .init("boot_method", "Boot method (optional)", required: false, options: bootMethods)]
    }

    private static let bmcPrefill = ["bmc_type": "bmc_type", "bmc_address": "bmc_address"]

    private static let bmcNote = " The password is stored encrypted by the controller, sent only over this app's pinned TLS session, and never shown again, copied or exported."

    static let setBmc = ResourceAction("Set BMC login…", "PUT", serverPath + "/bmc", destructive: true, controller: true, inputs: bmcFields(passwordRequired: false),
                                       prefill: bmcPrefill)
        .with {
            $0.adminOnly = true; $0.typeToConfirm = "hostname"
            $0.confirmTemplate = "Replaces the BMC type, address and user name stored for {hostname} (now {bmc_type} at {bmc_address}). Power, virtual media and serial console use these from now on." + bmcNote
        }

    static let rotateBmc = ResourceAction("Rotate BMC password…", "PUT", serverPath + "/bmc", destructive: true, controller: true, inputs: bmcFields(passwordRequired: true),
                                          prefill: bmcPrefill)
        .with {
            $0.adminOnly = true; $0.typeToConfirm = "hostname"
            $0.confirmTemplate = "Replaces the stored BMC password for {hostname}. Change it on the BMC itself first, or power and console calls will start failing." + bmcNote
        }

    static let deploy = ResourceAction("Deploy image…", serverPath + "/deploy", destructive: true, controller: true, inputs: [
        .init("os", "Installer or image name", initial: "ubuntu-24.04", check: .osName),
        .init("ssh_authorized_keys", "SSH public keys (one per line, optional)", .lines, required: false),
        .init("timezone", "Time zone", initial: "UTC", required: false, check: .timezone),
        .init("ttl_hours", "Install token lifetime in hours (0 = default)", .int, initial: "0", required: false, range: 0...168),
    ])
        .with {
            $0.adminOnly = true; $0.typeToConfirm = "hostname"
            $0.confirmTemplate = """
                Installs an operating system on {hostname} (MAC {mac_address}, {cpu_cores} CPU cores, {memory_mib} MiB). It will overwrite:
                - every disk the installer selects (existing data is lost)
                - the machine's lifecycle ({lifecycle} becomes deploying)
                - the next boot: it must PXE-boot to install, then it reboots and joins as a host
                The server must be allocated and have a MAC address; the controller refuses otherwise.
                """
        }

    static let mediaInsert = ResourceAction("Insert virtual media…", serverPath + "/virtual-media", destructive: true, controller: true,
                                            staticBody: .object(["action": .string("insert")]), inputs: [
        .init("url", "Image URL (empty = the controller's iPXE ISO)", .text, required: false, check: .mediaURL),
    ])
        .with {
            $0.adminOnly = true; $0.typeToConfirm = "hostname"
            $0.confirmTemplate = """
                Mounts an image as the virtual CD of {hostname} through its BMC ({bmc_address}). It will overwrite:
                - any media already mounted in that virtual drive
                - the machine's boot behaviour if it is set to boot from the CD
                The BMC downloads the image itself; use a URL it can reach and trust.
                """
        }

    static let mediaEject = ResourceAction("Eject virtual media", serverPath + "/virtual-media", destructive: true, controller: true,
                                           staticBody: .object(["action": .string("eject")]))
        .with {
            $0.adminOnly = true; $0.typeToConfirm = "hostname"
            $0.confirmTemplate = "Unmounts whatever image is in the virtual CD of {hostname}. A running installer or rescue system that reads it will fail."
        }

    /// Added to the Servers page. Serial-over-LAN is a button (`ResourceSpec.Accessory.bareMetalSol`), not an action.
    public static let writeActions: [ResourceAction] = [setBmc, rotateBmc, deploy, mediaInsert, mediaEject]

    // MARK: Serial-over-LAN

    public struct SolSession: Sendable, Equatable { public var url: URL; public var serverId: String }

    /// The SOL WebSocket on the daemon's own origin (the daemon proxies it to the controller, so the pinned
    /// certificate applies), wss when the daemon is https. `port` overrides the daemon's port for older setups.
    public static func solURL(wsPath: String, serverId: String, server: URL, port: Int? = nil) throws -> URL {
        guard let q = wsPath.firstIndex(of: "?") else { throw BareMetalError.badSolPath }
        let path = String(wsPath[..<q]), query = String(wsPath[wsPath.index(after: q)...])
        guard path == "/ws/v1/baremetal/\(serverId)/sol", query.hasPrefix("token="), !query.dropFirst(6).isEmpty,
              !query.contains("&"), !query.contains("#") else { throw BareMetalError.badSolPath }
        var c = URLComponents()
        c.scheme = server.scheme == "http" ? "ws" : "wss"
        c.host = server.host
        c.port = port ?? server.port
        c.percentEncodedPath = path
        c.percentEncodedQuery = query
        guard let u = c.url else { throw BareMetalError.badSolPath }
        return u
    }

    /// The SOL gate: IPMI with a stored login (the controller checks again and audits the token).
    public static func solBlocker(row: ResourceRow) -> String? {
        if row.values["bmc_type"]?.text != "ipmi" { return BareMetalError.notIPMI.errorDescription }
        if row.values["bmc_credentials_configured"] != .bool(true) { return BareMetalError.noBmcLogin.errorDescription }
        return nil
    }
}

public extension MachinaClient {
    /// Asks for a single-use SOL token (admin, audited) and returns the WebSocket URL. The token is in the URL: do not log it.
    func solURL(serverId: String, port: Int? = nil) async throws -> URL {
        let json = try await call(Endpoint("POST", "/api/v1/baremetal/servers/\(MachinaClient.seg(serverId))/sol-token", body: .object([:]), controller: true))
        guard let path = json["ws_path"]?.text else { throw BareMetalError.badSolPath }
        return try BareMetalOps.solURL(wsPath: path, serverId: serverId, server: server.url, port: port)
    }

    func openSol(serverId: String, port: Int? = nil) async throws -> WebSocketTransport {
        WebSocketTransport(session: urlSession, url: try await solURL(serverId: serverId, port: port), protocols: [], text: true)
    }
}
