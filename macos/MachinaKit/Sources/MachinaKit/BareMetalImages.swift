import Foundation

/// Bare-metal boot override, the verified image catalog and the OS picker for the deploy form.
/// Routes (controller `api/baremetal.rs`): `POST servers/{id}/boot-device` (admin), `GET|POST images` (admin to import),
/// `DELETE images/{name}` (admin). Images are never written unverified: the controller needs a checksum or a checksum file.
extension BareMetalOps {
    public static let bootDevices = ["pxe", "disk", "cdrom"]
    /// What follows a boot override: the override only matters at the next boot, and `cycle` also powers a stopped machine on.
    public static let powerSteps = ["cycle", "reset", "none"]
    public static let checksumAlgos = ["sha256", "sha512"]
    static let imagesPath = "/api/v1/baremetal/images"

    /// The deploy form's OS picker: the built-in installers plus every image that finished downloading and verifying.
    public static let osChoices = ResourceCreate.Choices(.controllerGet(imagesPath), rowsPath: ["images"], valueKeys: ["name"],
                                                         labelKeys: ["name"], extra: stockOS, only: ["status": "ready"])

    // MARK: Validation

    /// Mirrors the controller's `valid_name`: 1 to 64 of a-z, 0-9, '.', '-', '_', not starting with '.'.
    public static func imageNameProblem(_ s: String) -> String? {
        let ok = !s.isEmpty && s.count <= 64 && !s.hasPrefix(".")
            && s.allSatisfy { $0.isASCII && (($0.isLetter && $0.isLowercase) || $0.isNumber || ".-_".contains($0)) }
        return ok ? nil : "must be 1 to 64 characters of a-z, 0-9, . - _ (not starting with a dot)"
    }

    /// Image sources are fetched by the controller: https only, a host, and no login in the URL.
    public static func httpsURLProblem(_ s: String) -> String? {
        guard let u = URL(string: s), u.scheme?.lowercased() == "https" else { return "must be an https URL" }
        if (u.host ?? "").isEmpty { return "needs a host name" }
        if u.user != nil || u.password != nil || s.contains("@") { return "must not carry a login (user:pass@)" }
        if s.unicodeScalars.contains(where: { $0.value < 0x20 || $0.value == 0x7f }) || s.contains(" ") { return "contains characters that are not allowed" }
        return nil
    }

    /// A SHA-256 (64) or SHA-512 (128) hex digest.
    public static func hexDigestProblem(_ s: String) -> String? {
        let ok = (s.count == 64 || s.count == 128) && s.allSatisfy { $0.isHexDigit && $0.isASCII }
        return ok ? nil : "must be a SHA-256 (64) or SHA-512 (128 characters) hex digest"
    }

    // MARK: Boot override

    static let bootOverride = ResourceAction("Set boot device…", serverPath + "/boot-device", destructive: true, controller: true, inputs: [
        .init("device", "Next boot from", initial: "pxe", options: bootDevices),
        .init("persistent", "Keep until changed (otherwise the next boot only)", .bool, initial: "no"),
        .init("power_step", "Then power step (the override only applies at the next boot)", initial: "cycle", options: powerSteps),
    ])
        .with {
            $0.adminOnly = true; $0.typeToConfirm = "hostname"
            $0.followUp = .init("POST", serverPath + "/power", gate: "power_step", bodyKey: "action")
            $0.confirmTemplate = """
                Tells the BMC of {hostname} ({bmc_address}) which device to boot from next, then (unless you chose none) sends the power step. It will:
                - override the machine's boot order (Redfish and IPMI; a libvirt lab BMC cannot)
                - interrupt whatever runs on it when the step is cycle or reset; cycle also powers a stopped machine on
                Choose none to set only the override and power the machine yourself.
                """
        }

    // MARK: Image catalog

    private static let checksumNote = "The controller verifies every image against a checksum before marking it ready: give the hex digest, or an https checksum file (SHA256SUMS style) to look it up in."

    static let importImage = ResourceCreate("Import image from URL…", imagesPath, fields: [
        .init("name", "Image name (a-z, 0-9, . - _)", check: .imageName),
        .init("url", "Image URL (https)", check: .httpsURL),
        .init("checksum_algo", "Checksum algorithm", initial: "sha256", options: checksumAlgos),
        .init("checksum", "Checksum (hex digest)", required: false, check: .hexDigest),
        .init("sums_url", "Or checksum file URL (https)", required: false, check: .httpsURL),
        .init("os", "OS label (optional)", required: false, check: .osName),
        .init("arch", "Architecture", initial: "amd64", required: false, options: ["amd64", "arm64"]),
        .init("format", "Format", initial: "qcow2", required: false, options: ["qcow2", "raw"]),
    ])
        .with {
            $0.adminOnly = true; $0.requireAnyOf = ["checksum", "sums_url"]
            $0.warning = checksumNote + " The download runs in the background; the image shows as downloading until it passes."
        }

    static let deleteImage = ResourceAction("Delete image", "DELETE", imagesPath + "/{name}", destructive: true, controller: true)
        .with {
            $0.adminOnly = true; $0.typeToConfirm = "name"
            $0.confirmTemplate = "Deletes the image {name} from the controller, file included. Profiles and deploys that name it will fail until it is imported again."
        }

    static let importCatalogImage = ResourceAction("Import", imagesPath, destructive: true, controller: true, bodyFields: ["name": "name"])
        .with {
            $0.adminOnly = true
            $0.confirmTemplate = "Downloads {name} ({os}, {arch}) from its upstream URL in the background. The controller verifies it against the upstream checksum file before marking it ready."
        }

    /// Images already imported (status downloading, ready or failed).
    static let imported = ResourceSpec(
        "bm-images", "Images", group: "Bare metal", icon: "opticaldisc", endpoint: .controllerGet(imagesPath), rowsPath: ["images"], idKey: "name",
        columns: [.init("Name", "name"), .init("Status", "status"), .init("OS", "os"), .init("Arch", "arch"), .init("Format", "format"),
                  .init("Size", "size_bytes", .bytes), .init("Checksum", "checksum_algo"), .init("Digest", "checksum"), .init("Source", "source_url"),
                  .init("Error", "error"), .init("Added", "created_at", .date)],
        actions: [deleteImage], create: importImage, refreshSecs: 15)

    /// What the controller can fetch by name (upstream cloud images with their checksum files).
    static let catalog = ResourceSpec(
        "bm-image-catalog", "Image catalog", group: "Bare metal", icon: "shippingbox", endpoint: .controllerGet(imagesPath), rowsPath: ["catalog"], idKey: "name",
        columns: [.init("Name", "name"), .init("OS", "os"), .init("Arch", "arch"), .init("Format", "format"), .init("Checksum", "checksum_algo"), .init("URL", "url")],
        actions: [importCatalogImage])
}
