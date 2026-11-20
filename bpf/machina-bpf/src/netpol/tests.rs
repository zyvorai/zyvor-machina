// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use std::collections::BTreeMap;

use machina_bpf_common::IDENTITY_WORLD;

use super::*;

#[test]
fn cilium_backup_configs_are_not_cilium() {
    assert!(cni_config_name("05-cilium.conflist"));
    assert!(cni_config_name("05-cilium.conf"));
    assert!(!cni_config_name("00-multus.conf.cilium_bak"));
    assert!(!cni_config_name("05-cilium.conflist.bak"));
}

#[test]
fn vm_qos_rates_copy_into_edge_state() {
    let mut web = vm("web-1", "h1", "10.0.0.5", &[("app", "web")]);
    web.egress_mbps = 100;
    web.ingress_mbps = 50;
    web.pps = 1000;
    let c = compile(&Inputs {
        policies: &[],
        vms: &[web],
        services: &[],
        host: Some("h1"),
        host_addresses: &[],
        remote_node_addresses: &[],
    });
    let edge = c.state.vms.iter().find(|v| v.name == "web-1").unwrap();
    assert_eq!(edge.egress_mbps, 100);
    assert_eq!(edge.ingress_mbps, 50);
    assert_eq!(edge.pps, 1000);
}

fn vm(name: &str, host: &str, ip: &str, labels: &[(&str, &str)]) -> NetpolVm {
    NetpolVm {
        name: name.into(),
        host: Some(host.into()),
        project: Some("p1".into()),
        labels: labels
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect::<BTreeMap<_, _>>(),
        addresses: vec![ip.into()],
        ..Default::default()
    }
}

fn fleet() -> Vec<NetpolVm> {
    vec![
        vm(
            "web-1",
            "h1",
            "10.0.0.5",
            &[("app", "web"), ("env", "prod")],
        ),
        vm("web-2", "h2", "10.0.0.6", &[("app", "web"), ("env", "dev")]),
        vm(
            "db-1",
            "h1",
            "10.0.0.9",
            &[
                ("app", "db"),
                ("env", "prod"),
                ("machina.io/port.pg", "5432"),
            ],
        ),
        vm("lb-1", "h2", "10.0.0.2", &[("app", "lb")]),
    ]
}

fn parse(y: &str) -> Vec<VmNetworkPolicy> {
    let (p, v) = parse_documents(y);
    assert!(v.ok(), "{:?}", v.errors);
    p
}

fn trace_q(p: &[VmNetworkPolicy], from: &str, to: &str, proto: &str, port: u16) -> TraceResult {
    trace(
        p,
        &fleet(),
        &[],
        &["192.168.1.10".into()],
        &["192.168.1.11".into()],
        &TraceQuery {
            from: from.into(),
            to: to.into(),
            protocol: proto.into(),
            port,
            ..Default::default()
        },
    )
    .unwrap()
}

const WEB_DB: &str = r#"
apiVersion: machina.io/v1
kind: VmNetworkPolicy
metadata: {name: db}
spec:
  description: web may talk to db on pg
  endpointSelector: {matchLabels: {app: db}}
  ingress:
    - fromEndpoints: [{matchLabels: {app: web}}]
      toPorts: [{ports: [{port: pg, protocol: TCP}]}]
    - fromEntities: [host]
      icmps: [{fields: [{type: EchoRequest}]}]
"#;

#[test]
fn parse_multi_doc_and_cilium_kind() {
    let y = format!(
        "{WEB_DB}---\napiVersion: cilium.io/v2\nkind: CiliumNetworkPolicy\nmetadata: {{name: c, namespace: x}}\nspecs:\n  - endpointSelector: {{}}\n    egress: [{{toEntities: [world]}}]\n"
    );
    let (p, v) = parse_documents(&y);
    assert!(v.ok(), "{:?}", v.errors);
    assert_eq!(p.len(), 2);
    assert_eq!(
        p[1].annotations
            .get("machina.io/source-kind")
            .map(String::as_str),
        Some("CiliumNetworkPolicy")
    );
    assert!(v
        .warnings
        .iter()
        .any(|w| w.path.ends_with("metadata.namespace")));
    assert_eq!(
        p[0].description().as_deref(),
        Some("web may talk to db on pg")
    );
    let round = parse(&p[0].to_yaml());
    assert_eq!(round[0], p[0]);
}

#[test]
fn validation_paths() {
    let (_, v) = parse_documents(
        r#"
apiVersion: machina.io/v1
kind: VmNetworkPolicy
metadata: {name: Bad_Name}
spec:
  endpointSelector: {matchExpressions: [{key: a, operator: Maybe}]}
  ingress:
    - fromCIDR: [10.0.0.1]
      toPorts: [{ports: [{port: "80", endPort: 9000, protocol: TCP}]}]
      bogus: 1
  egress:
    - toFQDNs: [{matchName: example.com}, {matchName: "*.bad"}]
      toPorts: [{ports: [{port: "443"}], rules: {http: [{method: GET}]}}]
  egressDeny:
    - toFQDNs: [{matchName: example.com}]
"#,
    );
    let paths: Vec<&str> = v.errors.iter().map(|e| e.path.as_str()).collect();
    for want in [
        "document[0].metadata.name",
        "document[0].spec.endpointSelector.matchExpressions[0].operator",
        "document[0].spec.ingress[0].fromCIDR[0]",
        "document[0].spec.ingress[0].toPorts[0].ports[0].endPort",
        "document[0].spec.ingress[0].bogus",
        "document[0].spec.egress[0].toFQDNs[1].matchName",
        "document[0].spec.egressDeny[0].toFQDNs",
    ] {
        assert!(paths.contains(&want), "missing {want} in {paths:?}");
    }
    assert!(!v.warnings.iter().any(|w| w.path.contains("toFQDNs")));
    assert!(
        !v.warnings
            .iter()
            .any(|w| w.path.ends_with("toPorts[0].rules")),
        "L7 on toFQDNs is enforced"
    );
}

#[test]
fn compile_per_host_named_ports_and_icmp() {
    let p = parse(WEB_DB);
    let vms = fleet();
    let c = compile(&Inputs {
        policies: &p,
        vms: &vms,
        services: &[],
        host: Some("h1"),
        host_addresses: &[],
        remote_node_addresses: &[],
    });
    let names: Vec<&str> = c.state.vms.iter().map(|v| v.name.as_str()).collect();
    assert_eq!(names, ["db-1", "web-1"], "only h1 VMs are local");
    let db = vm_identity("db-1");
    let db_vm = c.state.vms.iter().find(|v| v.name == "db-1").unwrap();
    assert!(db_vm.isolate_ingress && !db_vm.isolate_egress);
    assert!(
        c.state
            .peers
            .iter()
            .any(|x| x.cidr == "10.0.0.6" && x.name == "web-2"),
        "remote VM is a peer"
    );
    let pg: Vec<_> = c.state.policy.iter().filter(|r| r.port == 5432).collect();
    assert_eq!(pg.len(), 2, "both web VMs (local and remote) reach pg");
    assert!(pg
        .iter()
        .all(|r| r.subject_identity == Some(db) && r.proto == 6 && !r.egress));
    assert!(c.state.policy.iter().any(|r| r.proto == 1
        && r.port == 9
        && r.peer_identity == Some(machina_bpf_common::IDENTITY_HOST)));
    let c2 = compile(&Inputs {
        policies: &p,
        vms: &vms,
        services: &[],
        host: Some("h2"),
        host_addresses: &[],
        remote_node_addresses: &[],
    });
    assert!(c2.state.policy.is_empty(), "no subjects on h2");
    assert!(c2
        .endpoints
        .iter()
        .any(|e| e.name == "db-1" && e.ingress_enforced));
}

#[test]
fn trace_allow_default_deny_and_deny_precedence() {
    let mut p = parse(WEB_DB);
    assert!(trace_q(&p, "web-2", "db-1", "TCP", 5432).allowed);
    let t = trace_q(&p, "lb-1", "db-1", "TCP", 5432);
    assert!(!t.allowed);
    assert_eq!(t.ingress.verdict, "default-deny");
    assert_eq!(
        trace_q(&p, "db-1", "lb-1", "TCP", 80).egress.verdict,
        "no-policy"
    );

    p.extend(parse(
        r#"
apiVersion: machina.io/v1
kind: VmNetworkPolicy
metadata: {name: no-dev}
spec:
  endpointSelector: {matchLabels: {app: db}}
  enableDefaultDeny: {ingress: false}
  ingressDeny:
    - fromEndpoints: [{matchLabels: {env: dev}}]
"#,
    ));
    let t = trace_q(&p, "web-2", "db-1", "TCP", 5432);
    assert!(!t.allowed, "deny beats the pg allow");
    assert_eq!(
        t.ingress.rule.as_deref(),
        Some("no-dev spec.ingressDeny[0]")
    );
    assert!(trace_q(&p, "web-1", "db-1", "TCP", 5432).allowed);
}

#[test]
fn cidr_except_world_and_requires() {
    let p = parse(
        r#"
apiVersion: machina.io/v1
kind: VmNetworkPolicy
metadata: {name: egress}
specs:
  - endpointSelector: {matchLabels: {app: web}}
    egress:
      - toCIDRSet: [{cidr: 203.0.113.0/24, except: [203.0.113.128/25]}]
        toPorts: [{ports: [{port: "443", endPort: 444, protocol: TCP}]}]
      - toEndpoints: [{}]
      - toEntities: [world]
        toPorts: [{ports: [{port: "53", protocol: UDP}]}]
  - endpointSelector: {matchLabels: {app: db}}
    ingress:
      - fromRequires: [{matchLabels: {env: prod}}]
      - fromEndpoints: [{matchLabels: {app: web}}]
"#,
    );
    assert!(trace_q(&p, "web-1", "203.0.113.10", "TCP", 444).allowed);
    assert!(
        !trace_q(&p, "web-1", "203.0.113.200", "TCP", 443).allowed,
        "except carves out the /25"
    );
    assert!(!trace_q(&p, "web-1", "198.51.100.1", "TCP", 443).allowed);
    assert!(trace_q(&p, "web-1", "198.51.100.1", "UDP", 53).allowed);
    assert!(
        trace_q(&p, "web-1", "203.0.113.200", "UDP", 53).allowed,
        "world covers CIDR identities"
    );
    assert!(
        trace_q(&p, "web-1", "lb-1", "TCP", 9999).allowed,
        "toEndpoints [{{}}] = every VM"
    );
    assert!(
        trace_q(&p, "web-1", "db-1", "TCP", 1).allowed,
        "prod web passes fromRequires"
    );
    let t = trace_q(&p, "web-2", "db-1", "TCP", 1);
    assert!(
        !t.allowed && t.ingress.verdict == "default-deny",
        "dev web lacks env=prod"
    );
    let w = trace_q(&p, "web-1", "world", "TCP", 443);
    assert_eq!(w.to.identity, IDENTITY_WORLD);
}

#[test]
fn empty_rule_denies_l4_only_allows_any() {
    let p = parse(
        r#"
apiVersion: machina.io/v1
kind: VmNetworkPolicy
metadata: {name: lock}
spec:
  endpointSelector: {matchLabels: {machina.io/vm-name: lb-1}}
  ingress: [{}]
  egress: [{toPorts: [{ports: [{port: "53", protocol: ANY}]}]}]
"#,
    );
    assert!(!trace_q(&p, "web-1", "lb-1", "TCP", 80).allowed);
    assert!(trace_q(&p, "lb-1", "1.1.1.1", "UDP", 53).allowed);
    assert!(trace_q(&p, "lb-1", "1.1.1.1", "SCTP", 53).allowed);
    assert!(!trace_q(&p, "lb-1", "1.1.1.1", "TCP", 80).allowed);
    let vms = fleet();
    let c = compile(&Inputs {
        policies: &p,
        vms: &vms,
        services: &[],
        host: None,
        host_addresses: &[],
        remote_node_addresses: &[],
    });
    let sel = c
        .selectors
        .iter()
        .find(|s| s.path == "spec.endpointSelector")
        .unwrap();
    assert_eq!(sel.vms, ["lb-1"]);
    assert_eq!(sel.selector, "machina.io/vm-name=lb-1");
}

#[test]
fn fqdn_rules_compile_and_trace() {
    let p = parse(
        r#"
apiVersion: cilium.io/v2
kind: CiliumNetworkPolicy
metadata: {name: dns-egress}
spec:
  endpointSelector: {matchLabels: {app: web}}
  egress:
    - toEndpoints: [{matchLabels: {app: db}}]
    - toEntities: [world]
      toPorts: [{ports: [{port: "53", protocol: UDP}]}]
    - toFQDNs: [{matchName: API.Example.com.}, {matchPattern: "**.cdn.example.net"}]
      toPorts: [{ports: [{port: "443", protocol: TCP}]}]
"#,
    );
    let vms = fleet();
    let c = compile(&Inputs {
        policies: &p,
        vms: &vms,
        services: &[],
        host: Some("h1"),
        host_addresses: &[],
        remote_node_addresses: &[],
    });
    assert!(c.warnings.is_empty(), "{:?}", c.warnings);
    let web1 = vm_identity("web-1");
    let pats: Vec<&str> = c.state.fqdn.iter().map(|r| r.pattern.as_str()).collect();
    assert_eq!(
        pats,
        ["**.cdn.example.net", "api.example.com"],
        "only the local web VM, normalized"
    );
    assert!(c
        .state
        .fqdn
        .iter()
        .all(|r| r.subject_identity == web1 && r.proto == 6 && r.port == 443));
    assert!(
        c.state
            .vms
            .iter()
            .find(|v| v.name == "web-1")
            .unwrap()
            .isolate_egress
    );

    let t = trace_q(&p, "web-1", "api.example.com", "TCP", 443);
    assert!(t.allowed, "{}", t.summary);
    assert_eq!(t.to.kind, "fqdn");
    assert_eq!(t.egress.rule.as_deref(), Some("dns-egress spec.egress[2]"));
    assert!(trace_q(&p, "web-2", "a.b.cdn.example.net", "TCP", 443).allowed);
    assert!(!trace_q(&p, "web-1", "cdn.example.net", "TCP", 443).allowed);
    assert!(!trace_q(&p, "web-1", "api.example.com", "TCP", 80).allowed);
    assert!(
        !trace_q(&p, "web-1", "203.0.113.5", "TCP", 443).allowed,
        "an address is not a name"
    );
}

const GROUPS: &str = r#"
apiVersion: cilium.io/v2alpha1
kind: CiliumCIDRGroup
metadata:
  name: partner-nets
  labels: {tier: partner, machina.io/region: eu}
  annotations: {machina.io/group-id: sg-123}
spec:
  externalCIDRs: [198.51.100.0/24, "2001:db8::/64"]
---
apiVersion: machina.io/v1
kind: VmNetworkPolicy
metadata: {name: partners}
spec:
  endpointSelector: {matchLabels: {app: web}}
  egress:
    - toGroups: [{aws: {securityGroupsIds: [sg-123]}}]
      toPorts: [{ports: [{port: "443", protocol: TCP}]}]
    - toCIDRSet: [{cidrGroupRef: partner-nets}]
      toPorts: [{ports: [{port: "22", protocol: TCP}]}]
    - toGroups: [{machina: {labels: {tier: partner}, region: us}}]
      toPorts: [{ports: [{port: "25", protocol: TCP}]}]
"#;

#[test]
fn cidr_groups_and_to_groups() {
    let p = parse(GROUPS);
    assert!(p[0].is_cidr_group());
    assert_eq!(p[0].group_cidrs(), ["198.51.100.0/24", "2001:db8::/64"]);
    assert!(p[0].to_yaml().contains("cilium.io/v2alpha1"));
    assert!(
        trace_q(&p, "web-1", "198.51.100.7", "TCP", 443).allowed,
        "group id match"
    );
    assert!(
        trace_q(&p, "web-1", "198.51.100.7", "TCP", 22).allowed,
        "cidrGroupRef"
    );
    assert!(
        !trace_q(&p, "web-1", "198.51.100.7", "TCP", 25).allowed,
        "region us does not match eu"
    );
    assert!(!trace_q(&p, "web-1", "203.0.113.1", "TCP", 443).allowed);
    let vms = fleet();
    let c = compile(&Inputs {
        policies: &p,
        vms: &vms,
        services: &[],
        host: Some("h1"),
        host_addresses: &[],
        remote_node_addresses: &[],
    });
    assert!(c.state.peers.iter().any(|x| x.cidr == "198.51.100.0/24"));
    assert!(
        c.endpoints.iter().all(|e| e.name != "partner-nets"),
        "a group selects no VMs"
    );

    let (_, v) = parse_documents(
        "apiVersion: cilium.io/v2alpha1\nkind: CiliumCIDRGroup\nmetadata: {name: g}\nspec: {externalCIDRs: [nope]}\n",
    );
    assert!(
        v.errors.iter().any(|e| e.path.contains("externalCIDRs[0]")),
        "{:?}",
        v.errors
    );
}

#[test]
fn authentication_modes() {
    let p = parse(
        r#"
apiVersion: cilium.io/v2
kind: CiliumNetworkPolicy
metadata: {name: auth}
spec:
  endpointSelector: {matchLabels: {app: db}}
  ingress:
    - fromEndpoints: [{matchLabels: {app: web}}]
      authentication: {mode: required}
    - fromEndpoints: [{matchLabels: {app: lb}}]
      authentication: {mode: test-always-fail}
"#,
    );
    let vms = fleet();
    let c = compile(&Inputs {
        policies: &p,
        vms: &vms,
        services: &[],
        host: Some("h1"),
        host_addresses: &[],
        remote_node_addresses: &[],
    });
    let web1 = vm_identity("web-1");
    let lb = vm_identity("lb-1");
    assert!(c
        .state
        .policy
        .iter()
        .any(|r| r.peer_identity == Some(web1) && r.auth == crate::api::AUTH_REQUIRED));
    assert!(c
        .state
        .policy
        .iter()
        .any(|r| r.peer_identity == Some(lb) && r.auth == crate::api::AUTH_ALWAYS_FAIL));
    let t = trace_q(&p, "web-1", "db-1", "TCP", 5432);
    assert!(
        t.allowed && t.ingress.auth.as_deref() == Some("required"),
        "{t:?}"
    );
    assert!(t.summary.contains("authenticated"));
    let t = trace_q(&p, "lb-1", "db-1", "TCP", 5432);
    assert!(!t.allowed && t.ingress.verdict == "auth-failed", "{t:?}");

    let (_, v) = parse_documents(
        "apiVersion: machina.io/v1\nkind: VmNetworkPolicy\nmetadata: {name: a}\nspec:\n  endpointSelector: {}\n  ingress: [{authentication: {mode: maybe}}]\n",
    );
    assert!(
        v.errors
            .iter()
            .any(|e| e.path.ends_with("authentication.mode")),
        "{:?}",
        v.errors
    );
}

const L7: &str = r#"
apiVersion: cilium.io/v2
kind: CiliumNetworkPolicy
metadata: {name: api}
spec:
  endpointSelector: {matchLabels: {app: db}}
  ingress:
    - fromEndpoints: [{matchLabels: {app: web}}]
      toPorts:
        - ports: [{port: "80", protocol: TCP}]
          rules:
            http:
              - method: GET
                path: "/v1/.*"
              - method: POST
                path: /login
                headers: ["X-Token: abc"]
        - ports: [{port: "9092", protocol: TCP}]
          rules:
            kafka: [{role: produce, topic: orders}]
        - ports: [{port: "443", protocol: TCP}]
          serverNames: [db.example.com]
    - fromEndpoints: [{matchLabels: {app: lb}}]
      toPorts: [{ports: [{port: "80", protocol: TCP}]}]
---
apiVersion: cilium.io/v2
kind: CiliumNetworkPolicy
metadata: {name: dns}
spec:
  endpointSelector: {matchLabels: {app: web}}
  egress:
    - toEndpoints: [{}]
    - toEntities: [world]
      toPorts:
        - ports: [{port: "53", protocol: ANY}]
          rules: {dns: [{matchPattern: "*.example.com"}]}
    - toFQDNs: [{matchName: api.example.com}]
      toPorts:
        - ports: [{port: "80", protocol: TCP}]
          rules: {http: [{method: GET}]}
"#;

fn trace_l7(
    p: &[VmNetworkPolicy],
    from: &str,
    to: &str,
    proto: &str,
    port: u16,
    f: impl FnOnce(&mut TraceQuery),
) -> TraceResult {
    let mut q = TraceQuery {
        from: from.into(),
        to: to.into(),
        protocol: proto.into(),
        port,
        ..Default::default()
    };
    f(&mut q);
    trace(p, &fleet(), &[], &[], &[], &q).unwrap()
}

#[test]
fn l7_compile_and_trace() {
    let p = parse(L7);
    let vms = fleet();
    let c = compile(&Inputs {
        policies: &p,
        vms: &vms,
        services: &[],
        host: Some("h1"),
        host_addresses: &[],
        remote_node_addresses: &[],
    });
    assert!(c.warnings.is_empty(), "{:?}", c.warnings);
    let kinds: Vec<&str> = c.state.l7.iter().map(|r| r.rules.kind()).collect();
    for k in ["http", "kafka", "tls", "dns"] {
        assert!(kinds.contains(&k), "{k} missing in {kinds:?}");
    }
    assert!(c.state.policy.iter().any(|r| r.port == 80 && r.l7));
    assert!(
        c.state.policy.iter().any(|r| r.port == 80 && !r.l7),
        "lb-1 gets plain L4 on 80"
    );
    assert!(c
        .state
        .fqdn
        .iter()
        .any(|r| r.l7.as_ref().is_some_and(|l| l.kind() == "http")));
    assert!(
        trace_l7(&p, "web-1", "203.0.113.9", "TCP", 80, |_| {})
            .egress
            .verdict
            == "default-deny"
    );

    let t = trace_l7(&p, "web-1", "db-1", "TCP", 80, |_| {});
    assert!(
        t.allowed
            && t.ingress
                .l7
                .as_deref()
                .is_some_and(|s| s.starts_with("http rules apply")),
        "{t:?}"
    );
    let get = |m: &str, path: &str| {
        trace_l7(&p, "web-1", "db-1", "TCP", 80, |q| {
            q.http_method = Some(m.into());
            q.http_path = Some(path.into());
        })
    };
    assert!(get("GET", "/v1/users").allowed);
    let t = get("DELETE", "/v1/users");
    assert!(!t.allowed && t.ingress.verdict == "l7-denied", "{t:?}");
    assert!(!get("GET", "/v2").allowed, "path regex is anchored");
    assert!(!get("POST", "/login").allowed, "header required");
    assert!(
        trace_l7(&p, "web-1", "db-1", "TCP", 80, |q| {
            q.http_method = Some("POST".into());
            q.http_path = Some("/login".into());
            q.http_headers = vec!["x-token: abc".into()];
        })
        .allowed
    );
    assert!(
        trace_l7(&p, "lb-1", "db-1", "TCP", 80, |q| q.http_method =
            Some("DELETE".into()))
        .allowed,
        "no L7 for lb"
    );

    let kafka = |key: &str, topic: &str| {
        trace_l7(&p, "web-1", "db-1", "TCP", 9092, |q| {
            q.kafka_api_key = Some(key.into());
            q.kafka_topic = Some(topic.into());
        })
        .allowed
    };
    assert!(kafka("produce", "orders"));
    assert!(!kafka("produce", "payments"));
    assert!(!kafka("fetch", "orders"));

    let tls = |sni: &str| {
        trace_l7(&p, "web-1", "db-1", "TCP", 443, |q| {
            q.server_name = Some(sni.into())
        })
        .allowed
    };
    assert!(tls("db.example.com"));
    assert!(!tls("evil.example.com"));

    let dns = |n: &str| {
        trace_l7(&p, "web-1", "1.1.1.1", "UDP", 53, |q| {
            q.dns_name = Some(n.into())
        })
        .allowed
    };
    assert!(dns("api.example.com"));
    assert!(!dns("example.org"));

    let t = trace_l7(&p, "web-1", "db-1", "TCP", 80, |q| {
        q.http_method = Some("GET".into())
    });
    assert!(
        !t.allowed && t.egress.verdict == "allowed",
        "path missing: GET / fails /v1/.*"
    );
    let fq = |m: &str| {
        trace_l7(&p, "web-1", "api.example.com", "TCP", 80, |q| {
            q.http_method = Some(m.into())
        })
        .allowed
    };
    assert!(fq("GET"));
    assert!(!fq("PUT"));
}

#[test]
fn to_services() {
    let svc =
        |name: &str, ns: &str, labels: &[(&str, &str)], eps: &[(&str, u16, u8)]| NetpolService {
            name: name.into(),
            namespace: ns.into(),
            labels: labels
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            endpoints: eps
                .iter()
                .map(|(a, port, proto)| ServiceEndpoint {
                    address: a.to_string(),
                    port: *port,
                    proto: *proto,
                })
                .collect(),
        };
    let services = vec![
        svc(
            "pg",
            "shop",
            &[("tier", "data")],
            &[("10.96.0.20", 5432, 6), ("10.0.0.9", 5432, 6)],
        ),
        svc(
            "dns",
            "kube-system",
            &[],
            &[("10.96.0.10", 53, 17), ("10.96.0.10", 53, 6)],
        ),
    ];
    let run = |y: &str, from: &str, to: &str, proto: &str, port: u16| {
        let p = parse(y);
        let q = TraceQuery {
            from: from.into(),
            to: to.into(),
            protocol: proto.into(),
            port,
            ..Default::default()
        };
        trace(&p, &fleet(), &services, &[], &[], &q)
            .unwrap()
            .allowed
    };
    let by_name = r#"
apiVersion: machina.io/v1
kind: VmNetworkPolicy
metadata: {name: s}
spec:
  endpointSelector: {matchLabels: {app: web}}
  egress: [{toServices: [{k8sService: {serviceName: pg, namespace: shop}}]}]
"#;
    assert!(run(by_name, "web-1", "10.96.0.20", "TCP", 5432));
    assert!(run(by_name, "web-1", "db-1", "TCP", 5432));
    assert!(!run(by_name, "web-1", "db-1", "TCP", 22));
    assert!(!run(by_name, "web-1", "10.96.0.20", "UDP", 5432));
    assert!(!run(by_name, "web-1", "10.96.0.10", "UDP", 53));

    let by_selector = r#"
apiVersion: machina.io/v1
kind: VmNetworkPolicy
metadata: {name: s}
spec:
  endpointSelector: {matchLabels: {app: web}}
  egress:
  - toServices: [{k8sServiceSelector: {selector: {matchLabels: {tier: data}}}}]
  - toServices: [{k8sService: {serviceName: dns}}]
    toPorts: [{ports: [{port: "53", protocol: UDP}, {port: "80", protocol: TCP}]}]
"#;
    assert!(run(by_selector, "web-1", "10.96.0.20", "TCP", 5432));
    assert!(run(by_selector, "web-1", "10.96.0.10", "UDP", 53));
    assert!(!run(by_selector, "web-1", "10.96.0.10", "TCP", 53));
    assert!(!run(by_selector, "web-1", "10.96.0.10", "TCP", 80));

    let wrong_ns = r#"
apiVersion: machina.io/v1
kind: VmNetworkPolicy
metadata: {name: s}
spec:
  endpointSelector: {matchLabels: {app: web}}
  egress: [{toServices: [{k8sService: {serviceName: pg, namespace: other}}]}]
"#;
    assert!(!run(wrong_ns, "web-1", "10.96.0.20", "TCP", 5432));
    let p = parse(wrong_ns);
    let vms = fleet();
    let c = compile(&Inputs {
        policies: &p,
        vms: &vms,
        services: &services,
        host: None,
        host_addresses: &[],
        remote_node_addresses: &[],
    });
    assert!(c.warnings.iter().any(|w| w.contains("selects no service")));

    let deny = r#"
apiVersion: machina.io/v1
kind: VmNetworkPolicy
metadata: {name: s}
spec:
  endpointSelector: {matchLabels: {app: web}}
  egress: [{toEntities: [all]}]
  egressDeny: [{toServices: [{k8sService: {serviceName: pg}}]}]
"#;
    assert!(!run(deny, "web-1", "db-1", "TCP", 5432));
    assert!(run(deny, "web-1", "db-1", "TCP", 22));

    let (_, v) = parse_documents(
        r#"
apiVersion: machina.io/v1
kind: VmNetworkPolicy
metadata: {name: s}
spec:
  endpointSelector: {}
  egress: [{toServices: [{k8sService: {}}, {k8sService: {serviceName: a}, k8sServiceSelector: {selector: {}}}]}]
"#,
    );
    assert!(!v.ok());
}

#[test]
fn jit_grant_opens_one_port_until_it_expires() {
    use chrono::TimeZone;
    let now = chrono::Utc.with_ymd_and_hms(2026, 10, 4, 12, 0, 0).unwrap();
    let req = jit::JitRequest {
        from: "lb-1".into(),
        to: "db-1".into(),
        port: 5432,
        secs: Some(600),
        reason: "migration".into(),
        ..Default::default()
    };
    let g = req.policy("alice", now).unwrap();
    let round = parse(&g.to_yaml());
    assert_eq!(round[0], g, "the grant is a valid policy");

    let mut p = parse(WEB_DB);
    assert!(!trace_q(&p, "lb-1", "db-1", "TCP", 5432).allowed);
    p.push(g.clone());
    assert!(trace_q(&p, "lb-1", "db-1", "TCP", 5432).allowed);
    assert!(!trace_q(&p, "lb-1", "db-1", "TCP", 22).allowed);
    assert!(
        trace_q(&p, "lb-1", "web-1", "TCP", 80).allowed,
        "the source is not egress-isolated by the grant"
    );
    assert!(
        trace_q(std::slice::from_ref(&g), "web-1", "db-1", "TCP", 22).allowed,
        "an open target stays open"
    );

    let soon = now + chrono::Duration::seconds(599);
    let gs = jit::grants(std::slice::from_ref(&g), soon);
    assert_eq!(gs.len(), 1);
    assert_eq!(
        (gs[0].from.as_str(), gs[0].to.as_str(), gs[0].port),
        ("lb-1", "db-1", 5432)
    );
    assert_eq!(gs[0].remaining_secs, 1);
    assert_eq!(gs[0].granted_by, "alice");
    let later = now + chrono::Duration::seconds(600);
    assert!(jit::expired(&g, later));
    assert!(jit::grants(&[g], later).is_empty());
}

#[test]
fn jit_rejects_bad_requests() {
    let now = chrono::Utc::now();
    let ok = jit::JitRequest {
        from: "host".into(),
        to: "db-1".into(),
        port: 22,
        ..Default::default()
    };
    let g = ok.policy("bob", now).unwrap();
    assert_eq!(g.specs.len(), 1, "host source: ingress only");
    let mut p = parse(WEB_DB);
    assert!(!trace_q(&p, "192.168.1.10", "db-1", "TCP", 22).allowed);
    p.push(g);
    assert!(trace_q(&p, "192.168.1.10", "db-1", "TCP", 22).allowed);
    for bad in [
        jit::JitRequest {
            from: String::new(),
            ..ok.clone()
        },
        jit::JitRequest {
            to: "host".into(),
            ..ok.clone()
        },
        jit::JitRequest {
            from: "db-1".into(),
            ..ok.clone()
        },
        jit::JitRequest {
            protocol: "icmp".into(),
            ..ok.clone()
        },
        jit::JitRequest {
            secs: Some(0),
            ..ok.clone()
        },
        jit::JitRequest {
            secs: Some(jit::JIT_MAX_SECS + 1),
            ..ok.clone()
        },
    ] {
        assert!(bad.policy("bob", now).is_err(), "{bad:?}");
    }
}
