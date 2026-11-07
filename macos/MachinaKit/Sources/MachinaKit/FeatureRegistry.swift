import Foundation

/// Read-only pages (Phase 1). Shapes come from live responses and `web/src/api/*.ts`.
public enum FeatureRegistry {
    public static let all: [ResourceSpec] = [
        ResourceSpec("node", "Host info", group: "Host", icon: "desktopcomputer", endpoint: .get("/api/v1/node"), layout: .keyValue),
        ResourceSpec("libvirt", "Libvirt", group: "Host", icon: "externaldrive.connected.to.line.below", endpoint: .get("/api/v1/libvirt/summary"), layout: .keyValue),
        ResourceSpec("health", "Health problems", group: "Host", icon: "stethoscope", endpoint: .get("/api/v1/health/problems"), rowsPath: ["items"]),
        ResourceSpec("services", "Services", group: "Host", icon: "gearshape.2", endpoint: .get("/api/v1/services"), idKey: "name",
                     columns: [.init("Name", "name"), .init("Description", "description"), .init("Active", "active_state"),
                               .init("Sub-state", "sub_state"), .init("Enabled", "enabled")]),
        ResourceSpec("audit", "Audit log", group: "Host", icon: "list.bullet.rectangle", endpoint: .get("/api/v1/audit", query: [.init(name: "limit", value: "200")]),
                     columns: [.init("Time", "timestamp", .date), .init("Action", "action"), .init("Target", "target"), .init("Result", "result")]),
        ResourceSpec("jobs", "Jobs", group: "Host", icon: "hourglass", endpoint: .get("/api/v1/jobs"), idKey: "id"),
        ResourceSpec("networks", "Networks", group: "Storage & network", icon: "network", endpoint: .get("/api/v1/networks"), idKey: "name",
                     columns: [.init("Name", "name"), .init("Bridge", "bridge"), .init("Active", "active"), .init("Autostart", "autostart"),
                               .init("Persistent", "persistent"), .init("UUID", "uuid")],
                     actions: [.init("Start", "/api/v1/networks/{name}/start"), .init("Stop", "/api/v1/networks/{name}/stop", destructive: true),
                               .init("Delete", "DELETE", "/api/v1/networks/{name}", destructive: true)]),
        ResourceSpec("pools", "Storage pools", group: "Storage & network", icon: "internaldrive", endpoint: .get("/api/v1/storage/pools"), idKey: "name",
                     columns: [.init("Name", "name"), .init("State", "state"), .init("Capacity", "capacity_gb", .gigabytes),
                               .init("Used", "allocation_gb", .gigabytes), .init("Free", "available_gb", .gigabytes), .init("Autostart", "autostart")],
                     actions: [.init("Start", "/api/v1/storage/pools/{name}/start"), .init("Stop", "/api/v1/storage/pools/{name}/stop", destructive: true),
                               .init("Refresh", "/api/v1/storage/pools/{name}/refresh")]),
        ResourceSpec("nwfilters", "Network filters", group: "Storage & network", icon: "line.3.horizontal.decrease.circle", endpoint: .get("/api/v1/nwfilters"),
                     idKey: "name", columns: [.init("Name", "name"), .init("UUID", "uuid")]),
        ResourceSpec("devices", "Host devices", group: "Storage & network", icon: "cpu", endpoint: .get("/api/v1/devices"), idKey: "name",
                     columns: [.init("Name", "name"), .init("Parent", "parent"), .init("Driver", "driver"), .init("Type", "capability_type")]),
        ResourceSpec("templates", "Templates", group: "VMs", icon: "doc.on.doc", endpoint: .get("/api/v1/templates"), idKey: "name",
                     columns: [.init("Name", "name"), .init("Description", "description"), .init("vCPUs", "vcpus"), .init("Memory MB", "memory_mb"),
                               .init("Disk GB", "disk_gb"), .init("OS", "os_variant")]),
        ResourceSpec("snapshots", "Snapshots", group: "VMs", icon: "camera.on.rectangle", endpoint: .get("/api/v1/snapshots"),
                     columns: [.init("VM", "vm_name"), .init("Name", "name"), .init("State", "state"), .init("Description", "description"),
                               .init("Parent", "parent"), .init("Current", "is_current")],
                     actions: [.init("Revert", "/api/v1/vms/{vm_name}/snapshots/{name}/revert", destructive: true),
                               .init("Delete", "DELETE", "/api/v1/vms/{vm_name}/snapshots/{name}", destructive: true)]),
        ResourceSpec("backups", "Backups", group: "VMs", icon: "externaldrive.badge.timemachine", endpoint: .get("/api/v1/backups"), idKey: "id",
                     columns: [.init("ID", "id"), .init("Time", "timestamp", .date), .init("VM", "vm_filter"), .init("Size", "size"),
                               .init("Status", "status"), .init("Disks", "with_disks")],
                     actions: [.init("Verify", "/api/v1/backups/{id}/verify"),
                               .init("Restore", "/api/v1/backups/restore", destructive: true, bodyFields: ["backup_id": "id"]),
                               .init("Delete", "DELETE", "/api/v1/backups/{id}", destructive: true)]),
        ResourceSpec("ha", "HA events", group: "Platform", icon: "arrow.triangle.2.circlepath", endpoint: .controllerGet("/api/v1/ha/status"), rowsPath: ["events"], idKey: "id",
                     columns: [.init("Time", "created_at", .date), .init("Action", "action"), .init("Message", "message"), .init("Host", "host_id")]),
    ]

    public static func spec(_ id: String) -> ResourceSpec? { all.first { $0.id == id } }

    /// Groups in display order.
    public static var groups: [(name: String, specs: [ResourceSpec])] {
        var order: [String] = []
        for s in all where !order.contains(s.group) { order.append(s.group) }
        return order.map { g in (g, all.filter { $0.group == g }) }
    }
}
