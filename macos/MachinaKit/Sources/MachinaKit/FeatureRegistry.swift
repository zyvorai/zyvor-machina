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

        // Containers (Vessel: Podman/Docker on the host)
        ResourceSpec("vessel", "Container engine", group: "Containers", icon: "shippingbox", endpoint: .get("/api/v1/vessel/status"), layout: .keyValue),
        ResourceSpec("containers", "Containers", group: "Containers", icon: "shippingbox.fill", endpoint: .get("/api/v1/vessel/containers"), rowsPath: ["items"], idKey: "id",
                     columns: [.init("Name", "name"), .init("Image", "image"), .init("State", "state"), .init("Status", "status"), .init("Created", "created", .date)],
                     actions: [.init("Start", "/api/v1/vessel/containers/{id}/start"),
                               .init("Stop", "/api/v1/vessel/containers/{id}/stop", destructive: true),
                               .init("Restart", "/api/v1/vessel/containers/{id}/restart", destructive: true),
                               .init("Remove", "DELETE", "/api/v1/vessel/containers/{id}", destructive: true, query: [.init(name: "force", value: "false")])]),
        ResourceSpec("pods", "Pods (Podman)", group: "Containers", icon: "square.stack.3d.up", endpoint: .get("/api/v1/vessel/pods"), rowsPath: ["items"], idKey: "id"),

        // Kubernetes (read-only; the cluster the daemon's kubeconfig reaches)
        ResourceSpec("k8s-overview", "Cluster overview", group: "Kubernetes", icon: "helm", endpoint: .get("/api/v1/k8s/overview"), layout: .keyValue),
        ResourceSpec("k8s-nodes", "Nodes", group: "Kubernetes", icon: "server.rack", endpoint: .get("/api/v1/k8s/nodes"), idKey: "name",
                     columns: [.init("Name", "name"), .init("Roles", "roles"), .init("Ready", "ready"), .init("Kubelet", "kubelet_version"),
                               .init("OS", "os_image"), .init("Kernel", "kernel_version"), .init("Runtime", "container_runtime")]),
        ResourceSpec("k8s-namespaces", "Namespaces", group: "Kubernetes", icon: "folder", endpoint: .get("/api/v1/k8s/namespaces"), rowsPath: ["items"], idKey: "metadata.name",
                     columns: [.init("Name", "metadata.name"), .init("Phase", "status.phase"), .init("Created", "metadata.creationTimestamp", .date)]),
        ResourceSpec("k8s-pods", "Pods (Kubernetes)", group: "Kubernetes", icon: "cube", endpoint: .get("/api/v1/k8s/pods"), rowsPath: ["items"], idKey: "metadata.uid",
                     columns: [.init("Name", "metadata.name"), .init("Namespace", "metadata.namespace"), .init("Phase", "status.phase"),
                               .init("Node", "spec.nodeName"), .init("Pod IP", "status.podIP"), .init("Created", "metadata.creationTimestamp", .date)]),
        ResourceSpec("k8s-deployments", "Deployments", group: "Kubernetes", icon: "square.stack.3d.forward.dottedline", endpoint: .get("/api/v1/k8s/deployments"), rowsPath: ["items"], idKey: "metadata.uid",
                     columns: [.init("Name", "metadata.name"), .init("Namespace", "metadata.namespace"), .init("Ready", "status.readyReplicas"),
                               .init("Desired", "spec.replicas"), .init("Created", "metadata.creationTimestamp", .date)]),
        ResourceSpec("k8s-services", "Services", group: "Kubernetes", icon: "point.3.connected.trianglepath.dotted", endpoint: .get("/api/v1/k8s/services"), rowsPath: ["items"], idKey: "metadata.uid",
                     columns: [.init("Name", "metadata.name"), .init("Namespace", "metadata.namespace"), .init("Type", "spec.type"),
                               .init("Cluster IP", "spec.clusterIP"), .init("Created", "metadata.creationTimestamp", .date)]),
        ResourceSpec("k8s-events", "Events", group: "Kubernetes", icon: "text.bubble", endpoint: .get("/api/v1/k8s/events"), rowsPath: ["items"], idKey: "metadata.uid",
                     columns: [.init("Last seen", "lastTimestamp", .date), .init("Type", "type"), .init("Reason", "reason"), .init("Object", "involvedObject.name"),
                               .init("Count", "count"), .init("Message", "message")]),

        // Controller: placement, access and security
        ResourceSpec("drs", "DRS consolidation", group: "Control plane", icon: "scale.3d", endpoint: .controllerGet("/api/v1/drs/consolidation"), layout: .keyValue),
        ResourceSpec("users", "Users", group: "Control plane", icon: "person.2", endpoint: .controllerGet("/api/v1/users"), idKey: "id",
                     columns: [.init("User", "username"), .init("Role", "role"), .init("Created", "created_at", .date)]),
        ResourceSpec("apikeys", "API keys", group: "Control plane", icon: "key", endpoint: .controllerGet("/api/v1/api-keys"), idKey: "id"),
        ResourceSpec("secgroups", "Security groups", group: "Control plane", icon: "lock.shield", endpoint: .controllerGet("/api/v1/security-groups"), idKey: "id"),
        ResourceSpec("fw-targets", "Firewall targets", group: "Control plane", icon: "flame", endpoint: .controllerGet("/api/v1/zeus-firewall/overview"), rowsPath: ["targets"], idKey: "id",
                     columns: [.init("Name", "name"), .init("Kind", "kind"), .init("Backend", "backend"), .init("Profile", "profile"), .init("Enabled", "enabled"),
                               .init("Risk", "risk"), .init("Score", "score"), .init("Open ports", "open_ports"), .init("Blocked today", "blocked_today")]),
        ResourceSpec("approvals", "Firewall approvals", group: "Control plane", icon: "checkmark.seal", endpoint: .controllerGet("/api/v1/zeus-firewall/approvals"), idKey: "id",
                     columns: [.init("Requested", "created_at", .date), .init("Target", "target_kind"), .init("Profile", "profile"), .init("Status", "status"),
                               .init("By", "requested_by"), .init("Reviewed by", "reviewed_by"), .init("Note", "review_note")],
                     actions: [.init("Approve", "/api/v1/zeus-firewall/approvals/{id}/approve", destructive: true, controller: true, staticBody: .object([:])),
                               .init("Reject", "/api/v1/zeus-firewall/approvals/{id}/reject", destructive: true, controller: true, staticBody: .object([:]))]),
    ]

    public static func spec(_ id: String) -> ResourceSpec? { all.first { $0.id == id } }

    /// Groups in display order.
    public static var groups: [(name: String, specs: [ResourceSpec])] {
        var order: [String] = []
        for s in all where !order.contains(s.group) { order.append(s.group) }
        return order.map { g in (g, all.filter { $0.group == g }) }
    }
}
