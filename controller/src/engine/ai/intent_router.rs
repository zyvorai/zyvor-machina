// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct SpotlightIntent {
    pub id: String,
    pub label: String,
    pub review: String,
    pub action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vm_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub navigate: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prefill: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchHit {
    pub kind: String,
    pub id: String,
    pub label: String,
    pub sublabel: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SpotlightResult {
    pub intents: Vec<SpotlightIntent>,
    pub search_hits: Vec<SearchHit>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggested_action: Option<SpotlightIntent>,
}

pub fn route_spotlight(query: &str, online_hosts: i64, vm_hits: Vec<SearchHit>) -> SpotlightResult {
    let q = query.trim();
    let mut intents = Vec::new();
    let search_hits = vm_hits;

    if q.is_empty() {
        return SpotlightResult {
            intents,
            search_hits,
            suggested_action: None,
        };
    }

    let ql = q.to_lowercase();

    if (ql.contains("guest")
        && (ql.contains("kernel") || ql.contains("logged in") || ql.contains("qga")))
        || ql.contains("old kernel")
        || (ql.contains("guest agent") && ql.contains("vm"))
    {
        intents.push(SpotlightIntent {
            id: "fleet-guest-query".into(),
            label: "Fleet guest-agent search".into(),
            review: format!("Analyze VMs matching: {q}"),
            action: "navigate".into(),
            vm_name: None,
            navigate: Some("/platform/vms".into()),
            prefill: Some(serde_json::json!({ "guestQuery": q })),
        });
    }

    if ql.contains("import") && ql.contains("network") {
        intents.push(intent(
            "import-networks",
            "Import networks",
            &format!("Import libvirt networks from {online_hosts} online host(s)."),
            "import_networks",
            None,
            None,
            None,
        ));
    }
    if ql.contains("import") && ql.contains("storage") {
        intents.push(intent(
            "import-storage",
            "Import storage",
            &format!("Discover storage pools from {online_hosts} online host(s)."),
            "import_storage",
            None,
            None,
            None,
        ));
    }
    if ql.contains("sync") && ql.contains("host") {
        intents.push(intent(
            "sync-hosts",
            "Sync hosts",
            "Queue inventory sync for all hypervisors.",
            "sync_hosts",
            None,
            None,
            None,
        ));
    }
    if ql.contains("offline") && ql.contains("host") {
        intents.push(intent(
            "show-offline-hosts",
            "Show offline hosts",
            "Navigate to hosts filtered to offline.",
            "navigate",
            None,
            Some("/platform/hosts?filter=offline".into()),
            None,
        ));
    }
    if ql.contains("mission control")
        || ql.contains("infrastructure earth")
        || (ql.contains("open") && ql.contains("mission"))
    {
        intents.push(intent(
            "open-mission-control",
            "Open Mission Control",
            "Infrastructure Earth — site, rack, and host overview.",
            "navigate",
            None,
            Some("/platform?mission=1".into()),
            None,
        ));
    }
    if ql.contains("overheat")
        || ql.contains("thermal")
        || (ql.contains("host") && ql.contains("pressure"))
    {
        intents.push(intent(
            "show-overheating-hosts",
            "Show hosts under pressure",
            "Open Mission Control filtered to thermal and resource pressure.",
            "navigate",
            None,
            Some("/platform?mission=1".into()),
            None,
        ));
    }
    if (ql.contains("kubevirt") || ql.contains("kubernetes vm"))
        && (ql.contains("discover") || ql.contains("inventory"))
    {
        intents.push(intent(
            "discovered-kubevirt-vms",
            "Discovered KubeVirt VMs",
            "Unmanaged KubeVirt guests in platform inventory.",
            "navigate",
            None,
            Some("/platform/vms?managed=false&source=kubevirt".into()),
            None,
        ));
    }
    if ql.contains("infrastructure health") || ql.contains("fleet health") {
        intents.push(intent(
            "infrastructure-health",
            "Infrastructure health",
            "Fleet health rollup in Mission Control.",
            "navigate",
            None,
            Some("/platform?mission=1".into()),
            None,
        ));
    }
    if ql.contains("disk pressure")
        || ql.contains("io pressure")
        || (ql.contains("host") && ql.contains("slow"))
    {
        intents.push(intent(
            "host-disk-pressure",
            "Host disk / IO pressure",
            "Open host Linux pane — PSI, SMART, thermal.",
            "navigate",
            None,
            Some("/platform/hosts".into()),
            None,
        ));
    }
    if ql.contains("disk utility")
        || ql.contains("smart fail")
        || (ql.contains("storage") && ql.contains("full"))
    {
        intents.push(intent(
            "disk-utility",
            "Disk Utility",
            "Storage pool capacity rings and hypervisor SMART disk health.",
            "navigate",
            None,
            Some("/platform/storage?tab=disks".into()),
            None,
        ));
    }
    if ql.contains("console")
        || ql.contains("fleet log")
        || ql.contains("audit log")
        || (ql.contains("task") && ql.contains("fail"))
    {
        intents.push(intent(
            "fleet-console",
            "Console",
            "Unified fleet log tail — audit, platform events, and task failures.",
            "navigate",
            None,
            Some("/platform/events".into()),
            None,
        ));
    }
    if ql.contains("stage manager")
        || ql.contains("workspace space")
        || ql.contains("workspace spaces")
        || (ql.contains("stage") && ql.contains("workspace"))
    {
        intents.push(intent(
            "fleet-spaces",
            "Stage Manager",
            "Workspace spaces strip — group VMs by tenant project.",
            "navigate",
            None,
            Some("/platform/projects".into()),
            None,
        ));
    }
    if ql.contains("general settings")
        || ql.contains("system settings general")
        || (ql.contains("appearance") && ql.contains("wallpaper"))
        || (ql.contains("customize") && ql.contains("dock"))
    {
        intents.push(intent(
            "fleet-general",
            "General",
            "System Settings — fleet desktop appearance, dock pins, and cluster summary.",
            "navigate",
            None,
            Some("/platform/settings?section=general".into()),
            None,
        ));
    }
    if ql.contains("shortcut")
        || (ql.contains("launchpad") && ql.contains("blueprint"))
        || (ql.contains("blueprint") && ql.contains("run"))
    {
        intents.push(intent(
            "fleet-shortcuts",
            "Shortcuts",
            "Blueprint Launchpad — one-click automation across VM sets.",
            "navigate",
            None,
            Some("/platform/blueprints?tab=launchpad".into()),
            None,
        ));
    }
    if ql.contains("add user")
        || ql.contains("create user")
        || ql.contains("new user")
        || ql.contains("add platform user")
    {
        intents.push(intent(
            "add-platform-user",
            "Add platform user",
            "Open Users settings to create an RBAC account (admin, operator, or viewer).",
            "navigate",
            None,
            Some("/platform/users".into()),
            None,
        ));
    }
    if ql.contains("users and groups")
        || ql.contains("users & groups")
        || ql.contains("workspace switch")
        || ql.contains("switch workspace")
        || ql.contains("switch tenant")
        || ql == "users"
        || ql.contains("manage users")
        || ql.contains("platform users")
    {
        intents.push(intent(
            "fleet-users",
            "Users & Groups",
            "Platform RBAC accounts and workspace (tenant) switcher.",
            "navigate",
            None,
            Some("/platform/users".into()),
            None,
        ));
    }
    if ql.contains("keychain")
        || ql.contains("secrets inventory")
        || ql.contains("api keys")
        || (ql.contains("vault") && ql.contains("list"))
    {
        intents.push(intent(
            "fleet-keychain",
            "Keychain",
            "Secrets inventory — vault providers, MFA policies, API keys, and air-gap bundles.",
            "navigate",
            None,
            Some("/platform/enterprise?tab=keychain".into()),
            None,
        ));
    }
    if ql.contains("maintenance mission")
        || ql.contains("patch timeline")
        || (ql.contains("maintenance") && ql.contains("mission"))
    {
        intents.push(intent(
            "maintenance-mission",
            "Maintenance Mission",
            "Guided 7-step patch timeline — scan, schedule, evacuate, preview, verify.",
            "navigate",
            None,
            Some("/platform/maintenance?tab=mission".into()),
            None,
        ));
    }
    if ql.contains("jarvis")
        && (ql.contains("shell") || ql.contains("landing") || ql.contains("home"))
    {
        intents.push(intent(
            "jarvis-shell",
            "Jarvis landing",
            "Intent-first desktop — Spotlight, dock, and quick fleet intents.",
            "navigate",
            None,
            Some("/platform".into()),
            None,
        ));
    }
    if ql.contains("software update")
        || ql.contains("host patch")
        || ql.contains("pending update")
        || (ql.contains("package") && ql.contains("update"))
    {
        intents.push(intent(
            "software-update",
            "Software Update",
            "Fleet host patch catalog — apt/dnf pending packages and reboot flags.",
            "navigate",
            None,
            Some("/platform/maintenance?tab=updates".into()),
            None,
        ));
    }
    if ql.contains("network lens")
        || ql.contains("reachability")
        || (ql.contains("why") && ql.contains("reach"))
    {
        intents.push(intent(
            "network-lens",
            "Network Lens",
            "Analyze VM-to-VM reachability — why can't A reach B?",
            "navigate",
            None,
            Some("/platform/networks?tab=lens".into()),
            None,
        ));
    }
    if ql.contains("systemd") && ql.contains("network") && ql.contains("host") {
        intents.push(intent(
            "host-network-settings",
            "Host network diagnostics",
            "systemd-networkd and resolved status on hypervisors.",
            "navigate",
            None,
            Some("/platform/settings".into()),
            None,
        ));
    }
    if ql.contains("guest") && (ql.contains("port") || ql.contains("exposed")) {
        intents.push(intent(
            "guest-ports-exposed",
            "Guest ports exposed",
            "Review guest firewall ports on VM detail.",
            "navigate",
            None,
            Some("/platform/vms".into()),
            None,
        ));
    }
    if ql.contains("firewall drift")
        || (ql.contains("fix") && ql.contains("firewall") && ql.contains("drift"))
    {
        intents.push(intent(
            "fix-firewall-drift",
            "Fix firewall drift",
            "Review Zeus Firewall targets with configuration drift.",
            "navigate",
            None,
            Some("/platform/zeus/security/firewall".into()),
            None,
        ));
    }
    if ql.contains("time machine") || (ql.contains("backup") && !ql.contains("restore")) {
        intents.push(intent(
            "time-machine-fleet",
            "Time Machine backups",
            "Fleet backup and snapshot timeline — restore with confidence.",
            "navigate",
            None,
            Some("/platform/backups".into()),
            None,
        ));
    }
    if ql.contains("smart folder")
        || ql.contains("finder")
        || (ql.contains("tag") && ql.contains("vm"))
    {
        intents.push(intent(
            "vm-finder",
            "VM Finder",
            "Smart folders and tag sidebar — browse VMs like Finder.",
            "navigate",
            None,
            Some("/platform/vms".into()),
            None,
        ));
    }
    if ql.contains("high cpu") && ql.contains("vm") {
        intents.push(intent(
            "finder-high-cpu",
            "High CPU VMs",
            "Smart folder — VMs above 85% CPU.",
            "navigate",
            None,
            Some("/platform/vms?folder=high_cpu".into()),
            None,
        ));
    }
    if let Some(name) = extract_after(&ql, "create vm ") {
        intents.push(intent(
            "create-vm",
            "Create VM",
            &format!("Open VM wizard for \"{name}\"."),
            "create_vm",
            Some(name.to_string()),
            None,
            None,
        ));
    } else if let Some(create) = parse_nl_create_vm(q) {
        intents.push(create);
    }
    if ql.contains("high cpu") || ql.contains("cpu") && ql.contains("consum") {
        intents.push(intent(
            "high-cpu",
            "High CPU VMs",
            "Open Activity Monitor sorted by CPU.",
            "navigate",
            None,
            Some("/platform/activity".into()),
            None,
        ));
    }
    if ql.contains("failed") && ql.contains("migrat") {
        intents.push(intent(
            "failed-migrations",
            "Failed migrations",
            "View migration tasks and jobs.",
            "navigate",
            None,
            Some("/platform/migration".into()),
            None,
        ));
    }
    if ql.contains("cost") || ql.contains("finops") {
        intents.push(intent(
            "cost-guardian",
            "Cost Guardian",
            "Open FinOps and rightsizing report.",
            "navigate",
            None,
            Some("/platform/reports".into()),
            None,
        ));
    }
    if ql.contains("exposure cost")
        || ql.contains("firewall waste")
        || ql.contains("port waste")
        || (ql.contains("exposure") && ql.contains("finops"))
    {
        intents.push(intent(
            "finops-exposure",
            "Exposure cost rollup",
            "Fleet firewall exposure waste and chargeback line items.",
            "navigate",
            None,
            Some("/platform/reports".into()),
            None,
        ));
        intents.push(intent(
            "finops-exposure-firewall",
            "Zeus Firewall FinOps",
            "Per-target exposure monthly cost and idle port ranking.",
            "navigate",
            None,
            Some("/platform/zeus/security/firewall".into()),
            None,
        ));
    }
    if ql.contains("capacity") || ql.contains("headroom") {
        intents.push(intent(
            "capacity",
            "Capacity Planner",
            "View cluster capacity and projections.",
            "navigate",
            None,
            Some("/platform/reports".into()),
            None,
        ));
    }
    if ql.contains("security") || ql.contains("risk") {
        intents.push(intent(
            "security",
            "Security Sentinel",
            "View security findings.",
            "navigate",
            None,
            Some("/platform/reports".into()),
            None,
        ));
    }
    if ql.contains("firewall settings") || ql.contains("firewall preference") {
        intents.push(intent(
            "zyra-fw-settings",
            "Firewall settings",
            "System Settings → Network → Firewall fleet summary and SLA.",
            "navigate",
            None,
            Some("/platform/settings".into()),
            None,
        ));
    }
    if ql.contains("block incoming") || ql.contains("block all incoming") {
        intents.push(intent(
            "zyra-block-incoming",
            "Block incoming connections",
            "Apply strict firewall profile or stealth mode on a machine.",
            "navigate",
            None,
            Some("/platform/zeus/security/firewall".into()),
            None,
        ));
    }
    if ql.contains("open ports") || ql.contains("open port") {
        intents.push(intent(
            "zyra-open-ports",
            "Open ports",
            "Review fleet port exposure and allowed services.",
            "navigate",
            None,
            Some("/platform/zeus/security/ports".into()),
            None,
        ));
    }
    if ql.contains("bare metal firewall")
        || ql.contains("bmc exposure")
        || ql.contains("ipmi exposed")
    {
        intents.push(intent(
            "zeus-metal-firewall",
            "Bare metal firewall",
            "BMC/PXE exposure and metal firewall profiles.",
            "navigate",
            None,
            Some("/platform/zeus/security/firewall".into()),
            None,
        ));
    }
    if ql.contains("pxe isolation") || ql.contains("provisioning network") {
        intents.push(intent(
            "zyra-pxe-isolation",
            "PXE isolation",
            "Apply BareMetalPxe profile on provisioning network.",
            "navigate",
            None,
            Some("/platform/zyra?tab=baremetal".into()),
            None,
        ));
    }
    if ql.contains("dr firewall") || ql.contains("multi-site") || ql.contains("multisite") {
        intents.push(intent(
            "zyra-multisite",
            "Multi-site DR firewall",
            "Federated policy export and cross-site profile sync.",
            "navigate",
            None,
            Some("/platform/zeus/security/firewall".into()),
            None,
        ));
    }
    if ql.contains("secure all hosts")
        || ql.contains("autonomous firewall")
        || ql.contains("ai operator")
    {
        intents.push(intent(
            "zyra-operator",
            "AI operator secure plan",
            "Guardrailed fleet secure-machine preview and approval gate.",
            "navigate",
            None,
            Some("/platform/zeus/security/firewall".into()),
            None,
        ));
    }
    if ql.contains("micro-segment") || ql.contains("micro segment") {
        intents.push(intent(
            "overlay-microseg",
            "Micro-segmentation overview",
            "Overlay segments, east-west policy, and compliance grade.",
            "navigate",
            None,
            Some("/platform/networks?tab=segments".into()),
            None,
        ));
    }
    if ql.contains("overlay network") || ql.contains("overlay segment") || ql.contains("nsx") {
        intents.push(intent(
            "overlay-network",
            "Overlay networks",
            "Tier-0/Tier-1 segments and libvirt network binding.",
            "navigate",
            None,
            Some("/platform/networks?tab=segments".into()),
            None,
        ));
    }
    if ql.contains("ipam") || (ql.contains("allocate") && ql.contains("ip")) {
        intents.push(intent(
            "overlay-ipam",
            "IPAM pools",
            "Allocate next-free addresses from segment pools.",
            "navigate",
            None,
            Some("/platform/networks?tab=ipam".into()),
            None,
        ));
    }
    if ql.contains("plugin marketplace")
        || ql.contains("install plugin")
        || (ql.contains("marketplace") && ql.contains("plugin"))
    {
        intents.push(intent(
            "marketplace-plugins",
            "Plugin marketplace",
            "Browse and install platform integration modules.",
            "navigate",
            None,
            Some("/platform/templates?tab=plugins".into()),
            None,
        ));
    }
    if ql.contains("storage tier")
        || ql.contains("vsan")
        || (ql.contains("backup") && ql.contains("sla"))
    {
        intents.push(intent(
            "storage-tiers",
            "Storage tiers & SLA",
            "vSAN-class gold/silver/bronze tiers and backup compliance.",
            "navigate",
            None,
            Some("/platform/storage?tab=tiers".into()),
            None,
        ));
    }
    if ql.contains("vault") || ql.contains("secrets provider") {
        intents.push(intent(
            "enterprise-vault",
            "Vault providers",
            "Secrets backend inventory and registration stubs.",
            "navigate",
            None,
            Some("/platform/settings".into()),
            None,
        ));
    }
    if ql.contains("mfa") || ql.contains("webauthn") || ql.contains("multi-factor") {
        intents.push(intent(
            "enterprise-mfa",
            "MFA policies",
            "Role-based WebAuthn/TOTP enrollment policy stubs.",
            "navigate",
            None,
            Some("/platform/settings".into()),
            None,
        ));
    }
    if ql.contains("air gap") || ql.contains("air-gap") || ql.contains("sovereign") {
        intents.push(intent(
            "enterprise-airgap",
            "Air-gap bundles",
            "Offline export manifest inventory for sovereign deployments.",
            "navigate",
            None,
            Some("/platform/settings".into()),
            None,
        ));
    }
    if ql.contains("runbook")
        && (ql.contains("catalog") || ql.contains("operations") || ql.contains("execute"))
    {
        intents.push(intent(
            "ops-runbooks",
            "Operations runbooks",
            "Catalog, execution history, and automated incident playbooks.",
            "navigate",
            None,
            Some("/platform/reports?tab=runbooks".into()),
            None,
        ));
    }
    if ql.contains("showback") || (ql.contains("compliance") && ql.contains("cost")) {
        intents.push(intent(
            "ops-showback",
            "Compliance showback",
            "Project cost + compliance grade rollup for chargeback.",
            "navigate",
            None,
            Some("/platform/reports?tab=showback".into()),
            None,
        ));
    }
    if ql.contains("firewall") || ql.contains("machine shield") {
        intents.push(intent(
            "zeus-firewall",
            "Zeus Firewall",
            "Fleet machine protection — open ports, profiles, lockdown.",
            "navigate",
            None,
            Some("/platform/zeus/security/firewall".into()),
            None,
        ));
    }
    if (ql.contains("secure")
        && (ql.contains("machine") || ql.contains("vm") || ql.contains("host")))
        || ql.contains("lock down")
        || ql.contains("lockdown")
    {
        intents.push(intent(
            "zyra-secure",
            "Secure this machine",
            "AI firewall plan with safe defaults and rollback.",
            "navigate",
            None,
            Some("/platform/zeus/security/firewall".into()),
            None,
        ));
    }
    if ql.contains("exposed")
        || (ql.contains("port") && ql.contains("5432"))
        || (ql.contains("database") && ql.contains("public"))
    {
        intents.push(intent(
            "zyra-exposure",
            "Open port exposure",
            "Scan fleet for critical database and SSH exposure.",
            "navigate",
            None,
            Some("/platform/zeus/security/ports".into()),
            None,
        ));
    }
    if ql.contains("firewall approval")
        || ql.contains("risky firewall")
        || ql.contains("approve firewall")
    {
        intents.push(intent(
            "zyra-fw-approval",
            "Firewall approvals",
            "Review pending risky firewall profile changes.",
            "navigate",
            None,
            Some("/platform/zeus/security/compliance".into()),
            None,
        ));
    }
    if ql.contains("gitops") && ql.contains("firewall") {
        intents.push(intent(
            "zyra-fw-gitops",
            "Firewall GitOps",
            "Export or sync MachineFirewallPolicy bundles.",
            "navigate",
            None,
            Some("/platform/zeus/security/compliance".into()),
            None,
        ));
    }
    if ql.contains("reach")
        || (ql.contains("can't") && ql.contains("connect"))
        || ql.contains("network path")
    {
        intents.push(intent(
            "network-lens",
            "Network Lens",
            "Analyze VM-to-VM connectivity on Topology.",
            "navigate",
            None,
            Some("/platform/topology".into()),
            None,
        ));
    }

    if let Some(vm_name) = extract_nl_vm_name(q) {
        if ql.contains("backup") || ql.contains("back up") {
            let label = format!("Backup VM {vm_name}");
            let review = format!("Queue a backup for {vm_name} — review before running.");
            intents.push(intent(
                "backup-vm",
                &label,
                &review,
                "backup_vm",
                Some(vm_name.clone()),
                None,
                None,
            ));
        }
        if ql.contains("migrate") {
            let target = extract_after(&ql, "to host ")
                .or_else(|| extract_after(&ql, " onto "))
                .or_else(|| extract_after(&ql, " to "))
                .map(|s| s.to_string());
            let review = match &target {
                Some(h) => {
                    format!("Migrate {vm_name} to host {h} — pre-check and review required.")
                }
                None => format!("Open migration workflow for {vm_name}."),
            };
            let label = format!("Migrate VM {vm_name}");
            intents.push(intent(
                "migrate-vm",
                &label,
                &review,
                "migrate_vm",
                Some(vm_name.clone()),
                None,
                target.map(|host| serde_json::json!({ "target_host": host })),
            ));
        }
        if (ql.contains("enable") || ql.contains("turn on")) && ql.contains("ha") {
            let label = format!("Enable HA for {vm_name}");
            let review =
                format!("Enable high availability policy for {vm_name} — review before applying.");
            intents.push(intent(
                "enable-ha-vm",
                &label,
                &review,
                "enable_ha_vm",
                Some(vm_name),
                None,
                None,
            ));
        }
    }

    if ql.contains("environment")
        || ql.contains("gpu cluster")
        || (ql.contains("staging") && (ql.contains("for") || ql.contains("developer")))
        || ql.contains("medium staging")
    {
        let plan = super::environment_intent::plan_environment(q, 0.04, 0.008);
        let label = plan.label.clone();
        let review = plan.review.clone();
        intents.push(intent(
            "environment-plan",
            &label,
            &review,
            "environment_plan",
            None,
            Some("/mission-control".into()),
            Some(serde_json::json!({ "query": q })),
        ));
        intents.push(intent(
            "environment-execute",
            "Build environment (preview)",
            "Preview VM creation for NL environment plan.",
            "navigate",
            None,
            Some("/mission-control".into()),
            None,
        ));
    }

    if (ql.contains("sre") && ql.contains("remediat")) || ql.contains("fix forecast") {
        intents.push(intent(
            "sre-remediate",
            "SRE remediations",
            "Proactive fixes from exhaustion forecasts.",
            "navigate",
            None,
            Some("/mission-control".into()),
            None,
        ));
    }

    if ql.contains("zyra") && (ql.contains("summary") || ql.contains("status")) {
        intents.push(intent(
            "zyra-summary",
            "Zyra OS summary",
            "Unified infrastructure OS health strip.",
            "navigate",
            None,
            Some("/platform/zyra".into()),
            None,
        ));
    }

    if ql.contains("power")
        && (ql.contains("waste") || ql.contains("carbon") || ql.contains("optimize"))
    {
        intents.push(intent(
            "fleet-power",
            "Fleet power optimizer",
            "Consolidate cold hosts and reduce power waste.",
            "navigate",
            None,
            Some("/platform/zyra".into()),
            None,
        ));
    }

    if ql.contains("what breaks")
        || ql.contains("shut down")
        || (ql.contains("shutdown") && ql.contains("host"))
        || ql.contains("evacuate")
        || (ql.contains("migrate") && ql.contains("host"))
    {
        if let Some(host_hint) = extract_after(&ql, "host ").or_else(|| extract_after(&ql, "down "))
        {
            let action = if ql.contains("evacuate") || ql.contains("migrate") {
                "migrate"
            } else {
                "shutdown"
            };
            let label = format!("Impact: {action} host {host_hint}");
            let review =
                format!("Simulate blast radius if host {host_hint} is evacuated or shut down.");
            intents.push(intent(
                "twin-impact",
                &label,
                &review,
                "twin_impact",
                None,
                Some("/platform/topology".into()),
                Some(serde_json::json!({
                    "action": action,
                    "target_kind": "host",
                    "target_id": host_hint,
                })),
            ));
        }
    }

    if ql.contains("isolate") && ql.contains("network") {
        intents.push(intent(
            "twin-network",
            "Network blast radius",
            "Simulate impact of isolating a network segment.",
            "twin_impact",
            None,
            Some("/platform/topology".into()),
            Some(serde_json::json!({
                "action": "isolate",
                "target_kind": "network",
                "target_id": "default",
            })),
        ));
    }

    if (ql.contains("storage") || ql.contains("pool"))
        && (ql.contains("drain") || ql.contains("shutdown"))
    {
        intents.push(intent(
            "twin-storage",
            "Storage pool blast radius",
            "Simulate impact of draining a storage pool.",
            "twin_impact",
            None,
            Some("/platform/topology".into()),
            Some(serde_json::json!({
                "action": "drain",
                "target_kind": "storage",
                "target_id": "default",
            })),
        ));
    }

    if ql.contains("remediation") && ql.contains("hub")
        || (ql.contains("unified") && ql.contains("remediat"))
        || (ql.contains("sre") && ql.contains("compliance") && ql.contains("fix"))
    {
        intents.push(intent(
            "remediate-hub",
            "Remediation hub",
            "Unified SRE, compliance, and fleet power actions.",
            "navigate",
            None,
            Some("/platform/zyra".into()),
            None,
        ));
    }

    if (ql.contains("runbook")
        && (ql.contains("diagnos") || ql.contains("knowledge") || ql.contains("slow")))
        || ql.contains("why is") && ql.contains("slow")
    {
        intents.push(intent(
            "knowledge-runbook",
            "Knowledge → runbook",
            "Diagnose query and generate operator runbook steps.",
            "navigate",
            None,
            Some("/platform/zyra".into()),
            None,
        ));
    }

    if (ql.contains("budget") || ql.contains("over spend") || ql.contains("finops guard"))
        && (ql.contains("cost") || ql.contains("spend") || ql.contains("budget"))
    {
        intents.push(intent(
            "cost-budget",
            "FinOps budget guard",
            "Monthly budget vs spend alerts and predictions.",
            "navigate",
            None,
            Some("/platform/reports".into()),
            None,
        ));
    }

    if ql.contains("stack")
        && (ql.contains("status") || ql.contains("running") || ql.contains("track"))
    {
        intents.push(intent(
            "mission-stack-status",
            "Mission stack status",
            "Track GPU and environment VMs from stack builds.",
            "navigate",
            None,
            Some("/mission-control".into()),
            None,
        ));
    }

    if ql.contains("migrate")
        && (ql.contains("doctor") || ql.contains("boot") || ql.contains("assurance"))
    {
        intents.push(intent(
            "guestkit-doctor",
            "GuestKit migration doctor",
            "Offline boot probability before cutover (qcow2/vmdk).",
            "navigate",
            None,
            Some("/platform/migration".into()),
            None,
        ));
    }

    if ql.contains("team")
        && (ql.contains("cost") || ql.contains("attribution") || ql.contains("chargeback"))
    {
        intents.push(intent(
            "cost-attribution",
            "Team cost attribution",
            "View FinOps breakdown by project and team tags.",
            "navigate",
            None,
            Some("/platform/reports".into()),
            None,
        ));
    }

    if ql.contains("rebalance")
        || ql.contains("drs")
        || (ql.contains("fleet") && ql.contains("hot"))
    {
        intents.push(intent(
            "fleet-rebalance",
            "Fleet rebalance",
            "Preview AI-driven live migrations to relieve hotspots.",
            "navigate",
            None,
            Some("/platform/zyra".into()),
            None,
        ));
    }

    if ql.contains("cis")
        || ql.contains("pci")
        || ql.contains("soc2")
        || ql.contains("hipaa")
        || (ql.contains("compliance") && ql.contains("framework"))
    {
        intents.push(intent(
            "compliance-frameworks",
            "Compliance frameworks",
            "Map cluster posture to CIS, PCI, SOC2, and HIPAA controls.",
            "navigate",
            None,
            Some("/platform/zyra".into()),
            None,
        ));
    }

    if ql.contains("llama") || ql.contains("gpu cluster") || ql.contains("inference stack") {
        let plan = super::mission_stack::plan_mission_stack(q, 0.04, 0.008);
        intents.push(intent(
            "mission-stack",
            &plan.label,
            &plan.review,
            "mission_stack",
            None,
            Some("/mission-control".into()),
            Some(serde_json::json!({ "query": q })),
        ));
        intents.push(intent(
            "mission-stack-execute",
            "Execute mission stack (preview)",
            "Preview GPU VM creation for infrastructure phase.",
            "navigate",
            None,
            Some("/mission-control".into()),
            None,
        ));
    }

    if ql.contains("why") && (ql.contains("slow") || ql.contains("down") || ql.contains("billing"))
    {
        intents.push(intent(
            "knowledge-diagnose",
            "Diagnose infrastructure issue",
            "NL root-cause hints from VMs, tasks, and metrics.",
            "navigate",
            None,
            Some("/platform/zyra".into()),
            Some(serde_json::json!({ "query": q })),
        ));
    }

    if ql.contains("gpu") && (ql.contains("place") || ql.contains("numa") || ql.contains("where")) {
        intents.push(intent(
            "gpu-placement",
            "GPU placement advisor",
            "Rank hosts for GPU / inference workloads.",
            "navigate",
            None,
            Some("/platform/zyra".into()),
            None,
        ));
    }

    if ql.contains("service")
        && (ql.contains("fail") || ql.contains("blast") || ql.contains("impact"))
    {
        intents.push(intent(
            "service-impact",
            "Service blast radius",
            "Simulate impact if an application service fails.",
            "navigate",
            None,
            Some("/platform/zyra".into()),
            None,
        ));
    }

    if ql.contains("similar") && ql.contains("incident") {
        intents.push(intent(
            "memory-similar",
            "Similar incidents",
            "Recall related audit and failure history.",
            "navigate",
            None,
            Some("/platform/topology".into()),
            None,
        ));
    }

    if ql.contains("audit log") || (ql.contains("audit") && ql.contains("controller")) {
        intents.push(intent(
            "audit-log",
            "Audit log",
            "Controller audit trail and fleet console stream.",
            "navigate",
            None,
            Some("/platform/events".into()),
            None,
        ));
    }
    if ql.contains("policy") && (ql.contains("quota") || ql.contains("rule")) {
        intents.push(intent(
            "policy-quotas",
            "Policy & quotas",
            "Project limits and controller policy rules.",
            "navigate",
            None,
            Some("/platform/policy".into()),
            None,
        ));
    }
    if ql.contains("guestkit") && ql.contains("job") {
        intents.push(intent(
            "guestkit-jobs",
            "GuestKit jobs",
            "Offline disk inspect and migrate-plan queue.",
            "navigate",
            None,
            Some("/platform/migration?tab=jobs".into()),
            None,
        ));
    }
    if ql.contains("firewall") && ql.contains("policy") {
        intents.push(intent(
            "firewall-policies",
            "Firewall Policy Studio",
            "Create, simulate, and sync Zeus firewall policies.",
            "navigate",
            None,
            Some("/platform/zeus/security/policies".into()),
            None,
        ));
    }
    if ql.contains("prometheus") || (ql.contains("metrics") && ql.contains("fleet")) {
        intents.push(intent(
            "fleet-prometheus",
            "Fleet Prometheus",
            "Scrape aggregate controller metrics exposition.",
            "navigate",
            None,
            Some("/platform/observability".into()),
            None,
        ));
    }
    if ql.contains("remediat") || (ql.contains("sre") && ql.contains("fix")) {
        intents.push(intent(
            "sre-remediate",
            "SRE remediations",
            "AI-suggested fixes from live fleet analysis.",
            "navigate",
            None,
            Some("/platform/recommendations".into()),
            None,
        ));
    }

    if (ql.contains("attack") && ql.contains("reach")) || ql.contains("attacker") {
        intents.push(intent(
            "attack-path",
            "Security attack path",
            "Analyze how an attacker could reach a workload VM.",
            "navigate",
            None,
            Some("/platform/zyra".into()),
            None,
        ));
    }

    if ql.contains("graph")
        || (ql.contains("reach") && ql.contains("vm"))
        || ql.contains("connectivity")
    {
        intents.push(intent(
            "infra-graph",
            "Infrastructure Graph Brain",
            "Unified VM, network, and firewall path analysis.",
            "navigate",
            None,
            Some("/platform/zyra?tab=brain".into()),
            None,
        ));
    }
    if ql.contains("rightsize")
        || ql.contains("oversized")
        || (ql.contains("idle") && ql.contains("vm"))
    {
        intents.push(intent(
            "rightsizing",
            "VM Rightsizing",
            "FinOps recommendations from fleet metrics.",
            "navigate",
            None,
            Some("/platform/zyra/rightsizing".into()),
            None,
        ));
    }
    if ql.contains("incident") || ql.contains("war room") || ql.contains("commander") {
        intents.push(intent(
            "incident-commander",
            "Incident Commander",
            "Correlated war room for active outages.",
            "navigate",
            None,
            Some("/platform/zyra/incidents".into()),
            None,
        ));
    }
    if ql.contains("troubleshoot")
        || (ql.contains("slow") && ql.contains("vm"))
        || ql.contains("vm is slow")
    {
        intents.push(intent(
            "troubleshoot-vm",
            "Troubleshoot VM",
            "Multi-domain diagnosis: CPU, memory, disk, network, host pressure.",
            "nl_ops",
            None,
            None,
            Some(serde_json::json!({ "query": query })),
        ));
    }
    if (ql.contains("create") && (ql.contains("vm") || ql.contains("ubuntu")))
        || (ql.contains("migrate") && ql.contains("from"))
        || ql.contains("risky infra")
        || (ql.contains("storage") && ql.contains("slow"))
    {
        intents.push(intent(
            "nl-ops",
            "Execute fleet operation",
            "Dry-run plan with approval queue for mutating changes.",
            "nl_ops",
            None,
            None,
            Some(serde_json::json!({ "query": query })),
        ));
    }

    let suggested_action = intents.first().cloned();
    SpotlightResult {
        intents,
        search_hits,
        suggested_action,
    }
}

pub fn intent(
    id: &str,
    label: &str,
    review: &str,
    action: &str,
    vm_name: Option<String>,
    navigate: Option<String>,
    prefill: Option<serde_json::Value>,
) -> SpotlightIntent {
    SpotlightIntent {
        id: id.into(),
        label: label.into(),
        review: review.into(),
        action: action.into(),
        vm_name,
        navigate,
        prefill,
    }
}

/// Curated navigate intents for Jarvis landing (Phase 57 v1).
pub fn jarvis_landing_intents(
    online_hosts: i64,
    missing_images: Vec<crate::engine::template_readiness::MissingTemplateImage>,
) -> SpotlightResult {
    let mut intents = vec![
        intent(
            "jarvis-mission-control",
            "Mission Control",
            "Canvas Infrastructure Earth globe, site legend, and rack geography.",
            "navigate",
            None,
            Some("/platform?mission=1".into()),
            None,
        ),
        intent(
            "jarvis-maintenance-mission",
            "Maintenance Mission",
            "Guided 7-step patch timeline — scan, schedule, evacuate, verify.",
            "navigate",
            None,
            Some("/platform/maintenance?tab=mission".into()),
            None,
        ),
        intent(
            "jarvis-machine-finder",
            "Machine Finder",
            "Browse DC → rack → host → VM geography.",
            "navigate",
            None,
            Some("/platform/hosts/finder".into()),
            None,
        ),
        intent(
            "jarvis-gpu",
            "GPU Command Center",
            "MIG, vGPU, and CUDA inventory with placement advisor.",
            "navigate",
            None,
            Some("/platform/gpu".into()),
            None,
        ),
        intent(
            "jarvis-operations",
            "Operations hub",
            "Runbooks, showback, tasks, and fleet alerts.",
            "navigate",
            None,
            Some("/platform/operations".into()),
            None,
        ),
        intent(
            "jarvis-offline-hosts",
            "Offline hosts",
            "Review hypervisors that missed heartbeat.",
            "navigate",
            None,
            Some("/platform/hosts?filter=offline".into()),
            None,
        ),
        intent(
            "jarvis-import-networks",
            "Import networks",
            &format!("Import libvirt networks from {online_hosts} online host(s)."),
            "navigate",
            None,
            Some("/platform/networks".into()),
            None,
        ),
        intent(
            "jarvis-zyra",
            "Zyra OS",
            "Cloud layer — K8s, firewall, bare metal, and AI workloads.",
            "navigate",
            None,
            Some("/platform/zyra".into()),
            None,
        ),
        intent(
            "jarvis-enterprise",
            "Enterprise Keychain",
            "Vault, MFA, FIPS matrix, and tenant isolation inventory.",
            "navigate",
            None,
            Some("/platform/enterprise?tab=keychain".into()),
            None,
        ),
    ];
    if !missing_images.is_empty() {
        let auto = missing_images.iter().filter(|m| m.auto_fetch).count();
        let names: Vec<String> = missing_images
            .iter()
            .take(4)
            .map(|m| m.name.clone())
            .collect();
        let suffix = if missing_images.len() > 4 {
            format!(" +{} more", missing_images.len() - 4)
        } else {
            String::new()
        };
        intents.insert(
            0,
            intent(
                "jarvis-missing-images",
                "Missing golden images",
                &format!(
                    "{} catalog disk(s) not on hosts ({}{}) — {} auto-download on first VM create.",
                    missing_images.len(),
                    names.join(", "),
                    suffix,
                    auto,
                ),
                "navigate",
                None,
                Some("/platform/templates".into()),
                None,
            ),
        );
    }
    let suggested_action = intents.first().cloned();
    SpotlightResult {
        intents,
        search_hits: vec![],
        suggested_action,
    }
}

fn extract_after<'a>(hay: &'a str, needle: &str) -> Option<&'a str> {
    let idx = hay.find(needle)?;
    let rest = hay[idx + needle.len()..].trim();
    if rest.is_empty() {
        None
    } else {
        Some(rest.split_whitespace().next().unwrap_or(rest))
    }
}

fn parse_nl_create_vm(query: &str) -> Option<SpotlightIntent> {
    let ql = query.to_lowercase();
    let wants_vm = ql.contains("create")
        && (ql.contains("vm")
            || ql.contains("vcpu")
            || ql.contains("windows")
            || ql.contains("linux")
            || ql.contains("ubuntu"));
    if !wants_vm {
        return None;
    }

    let cores = extract_number_before(&ql, &["vcpu", "vcpus", "cpu", "cpus", "core", "cores"]);
    let memory_gib = extract_memory_gib(&ql);
    let os = crate::engine::template_catalog::default_template_for_natural_language(&ql);

    let name = extract_quoted_name(query)
        .or_else(|| extract_after(&ql, "named ").map(|s| s.to_string()))
        .or_else(|| extract_after(&ql, "called ").map(|s| s.to_string()))
        .or_else(|| {
            query
                .split_whitespace()
                .find(|w| w.contains('-') && w.len() > 2)
                .map(|s| {
                    s.trim_matches(|c: char| !c.is_alphanumeric() && c != '-')
                        .to_string()
                })
        })
        .unwrap_or_else(|| "new-vm".into());

    let size = match (cores, memory_gib) {
        (Some(c), Some(m)) if c >= 8 || m >= 16 => "large",
        (Some(c), Some(m)) if c >= 4 || m >= 8 => "medium",
        _ => "small",
    };

    let review = match (cores, memory_gib) {
        (Some(c), Some(m)) => format!("Create {name} — {c} vCPU, {m} GiB, {os}."),
        (Some(c), None) => format!("Create {name} — {c} vCPU, {os}."),
        (None, Some(m)) => format!("Create {name} — {m} GiB RAM, {os}."),
        _ => format!("Create {name} with {os}."),
    };

    let prefill = serde_json::json!({
        "name": name,
        "os": os,
        "size": size,
        "network": "default",
        "cores": cores,
        "memory_gib": memory_gib,
    });

    Some(intent(
        "create-vm",
        "Create VM (NL)",
        &review,
        "create_vm",
        Some(name),
        None,
        Some(prefill),
    ))
}

fn extract_number_before(hay: &str, units: &[&str]) -> Option<i32> {
    for unit in units {
        if let Some(idx) = hay.find(unit) {
            let prefix = hay[..idx].trim();
            let num: String = prefix
                .chars()
                .rev()
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>()
                .chars()
                .rev()
                .collect();
            if let Ok(n) = num.parse::<i32>() {
                if n > 0 && n <= 128 {
                    return Some(n);
                }
            }
        }
    }
    None
}

fn extract_memory_gib(hay: &str) -> Option<i32> {
    for token in hay.split_whitespace() {
        let digits: String = token.chars().take_while(|c| c.is_ascii_digit()).collect();
        if let Ok(n) = digits.parse::<i32>() {
            let rest = token[digits.len()..].to_lowercase();
            if rest.starts_with("gb") || rest.starts_with("gib") || rest.starts_with("gi") {
                return Some(n.clamp(1, 1024));
            }
        }
    }
    None
}

fn extract_quoted_name(query: &str) -> Option<String> {
    for (open, close) in [('\"', '\"'), ('\'', '\'')] {
        if let Some(start) = query.find(open) {
            let rest = &query[start + 1..];
            if let Some(end) = rest.find(close) {
                let name = rest[..end].trim();
                if !name.is_empty() {
                    return Some(name.to_string());
                }
            }
        }
    }
    None
}

fn extract_nl_vm_name(query: &str) -> Option<String> {
    let ql = query.to_lowercase();
    if let Some(name) = extract_quoted_name(query) {
        return Some(name);
    }
    if let Some(rest) = extract_after(&ql, "vm ") {
        let token = rest
            .split_whitespace()
            .next()?
            .trim_matches(|c: char| !c.is_alphanumeric() && c != '-' && c != '_');
        if !token.is_empty() && token.len() <= 64 {
            return Some(token.to_string());
        }
    }
    query
        .split_whitespace()
        .find(|w| w.contains('-') && w.len() > 2)
        .map(|s| {
            s.trim_matches(|c: char| !c.is_alphanumeric() && c != '-' && c != '_')
                .to_string()
        })
}
