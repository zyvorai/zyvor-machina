// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Plain-English VM network policies. A small parser turns common sentences
//! ("only web can reach db on 5432", "block np-client from the internet")
//! into CiliumNetworkPolicy documents; the controller also asks its LLM,
//! with the prompt built here, and falls back to the parser.

use std::collections::BTreeMap;

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use super::{parse_documents, NetpolVm, VmNetworkPolicy, LABEL_VM_NAME};

/// Label put on every drafted policy.
pub const LABEL_SOURCE: &str = "machina.io/source";
pub const SOURCE_VALUE: &str = "plain-english";
/// Longest request accepted.
pub const MAX_PROMPT: usize = 4000;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Draft {
    pub yaml: String,
    #[serde(skip)]
    pub policies: Vec<VmNetworkPolicy>,
    /// `rules` (the parser) or `llm`.
    pub source: String,
    /// What each policy does, in words, plus caveats.
    pub notes: Vec<String>,
    /// Sentences the parser could not read.
    pub unparsed: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
enum Peer {
    Vms(BTreeMap<String, String>),
    All,
    Entity(&'static str),
    Cidr(String),
    Fqdn(String),
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Verb {
    Only,
    Allow,
    Deny,
}

#[derive(Debug, Clone, PartialEq)]
struct Port {
    port: u16,
    proto: &'static str,
}

fn service_ports(name: &str) -> Option<Vec<Port>> {
    let tcp = |p| {
        vec![Port {
            port: p,
            proto: "TCP",
        }]
    };
    Some(match name {
        "ssh" => tcp(22),
        "http" | "web traffic" => tcp(80),
        "https" | "tls" => tcp(443),
        "dns" => vec![
            Port {
                port: 53,
                proto: "UDP",
            },
            Port {
                port: 53,
                proto: "TCP",
            },
        ],
        "postgres" | "postgresql" | "psql" => tcp(5432),
        "mysql" | "mariadb" => tcp(3306),
        "redis" => tcp(6379),
        "mongodb" | "mongo" => tcp(27017),
        "rdp" => tcp(3389),
        "smtp" => tcp(25),
        "ntp" => vec![Port {
            port: 123,
            proto: "UDP",
        }],
        "kafka" => tcp(9092),
        _ => return None,
    })
}

/// `5432`, `ports 80 and 443`, `53/udp`, `udp 53`, `ssh`, `https and dns`.
fn parse_ports(s: &str) -> Result<Vec<Port>, String> {
    let s = s
        .trim()
        .trim_start_matches("the ")
        .replace("ports", " ")
        .replace("port", " ");
    let bare_udp = Regex::new(r"(^|\s)udp(\s|$)").unwrap().is_match(&s);
    let bare_sctp = Regex::new(r"(^|\s)sctp(\s|$)").unwrap().is_match(&s);
    let mut out = Vec::new();
    for tok in Regex::new(r"[,\s]+|\band\b|\bor\b")
        .unwrap()
        .split(&s)
        .map(str::trim)
        .filter(|t| !t.is_empty())
    {
        if matches!(tok, "tcp" | "udp" | "sctp" | "only") {
            continue;
        }
        if let Some(ps) = service_ports(tok) {
            out.extend(ps);
            continue;
        }
        let (num, proto) = match tok.split_once('/') {
            Some((a, b)) if a.parse::<u16>().is_ok() => (a, b),
            Some((a, b)) => (b, a),
            None => (tok, ""),
        };
        let proto = match proto {
            "tcp" => "TCP",
            "udp" => "UDP",
            "sctp" => "SCTP",
            "any" => "ANY",
            "" if bare_udp => "UDP",
            "" if bare_sctp => "SCTP",
            "" => "TCP",
            other => return Err(format!("unknown protocol `{other}`")),
        };
        let port: u16 = num
            .parse()
            .ok()
            .filter(|p| *p > 0)
            .ok_or_else(|| format!("`{tok}` is not a port or known service"))?;
        out.push(Port { port, proto });
    }
    if out.is_empty() {
        return Err(format!("no port in `{}`", s.trim()));
    }
    Ok(out)
}

const LABEL_KEYS: [&str; 5] = ["app", "role", "tier", "service", "component"];

fn peer(s: &str, inv: &[NetpolVm]) -> Option<Peer> {
    let s = s.trim().trim_matches(|c| c == '"' || c == '`' || c == '\'');
    let bare = s
        .strip_prefix("the ")
        .unwrap_or(s)
        .trim_start_matches("vm ")
        .trim();
    match bare {
        "anyone" | "anything" | "everyone" | "everything" | "any vm" | "any vms" | "all vms"
        | "every vm" | "other vms" | "any other vm" | "all other vms" => return Some(Peer::All),
        "host" | "hypervisor" | "host machine" => return Some(Peer::Entity("host")),
        "internet" | "world" | "anywhere" | "outside" | "outside world" | "public internet"
        | "external hosts" => return Some(Peer::Entity("world")),
        _ => {}
    }
    if let Ok(ip) = bare.parse::<std::net::IpAddr>() {
        let bits = if ip.is_ipv4() { 32 } else { 128 };
        return Some(Peer::Cidr(format!("{ip}/{bits}")));
    }
    if let Some((a, b)) = bare.split_once('/') {
        if a.parse::<std::net::IpAddr>().is_ok() && b.parse::<u8>().is_ok() {
            return Some(Peer::Cidr(bare.to_string()));
        }
    }
    let labels = Regex::new(
        r"^(?:vms? |machines? )?(?:labell?ed |with (?:the )?labels? |with |tagged )?((?:[a-z0-9._/-]+=[a-z0-9._-]+)(?:\s*,\s*[a-z0-9._/-]+=[a-z0-9._-]+)*)$",
    )
    .unwrap();
    if let Some(c) = labels.captures(bare) {
        let m = c[1]
            .split(',')
            .filter_map(|kv| kv.trim().split_once('='))
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        return Some(Peer::Vms(m));
    }
    if let Some(v) = inv.iter().find(|v| v.name.eq_ignore_ascii_case(bare)) {
        return Some(Peer::Vms(
            [(LABEL_VM_NAME.to_string(), v.name.clone())].into(),
        ));
    }
    let word = Regex::new(r"\s+(vms?|servers?|machines?|nodes?|boxes|instances?|tier)$")
        .unwrap()
        .replace(bare, "")
        .to_string();
    let candidates = [word.clone(), word.trim_end_matches('s').to_string()];
    for key in LABEL_KEYS {
        for w in &candidates {
            if inv
                .iter()
                .any(|v| v.labels.get(key).is_some_and(|x| x.eq_ignore_ascii_case(w)))
            {
                return Some(Peer::Vms([(key.to_string(), w.clone())].into()));
            }
        }
    }
    for w in &candidates {
        let hit = inv.iter().find_map(|v| {
            v.labels
                .iter()
                .find(|(k, x)| !k.starts_with("machina.io/") && x.eq_ignore_ascii_case(w))
                .map(|(k, x)| (k.clone(), x.clone()))
        });
        if let Some((k, x)) = hit {
            return Some(Peer::Vms([(k, x)].into()));
        }
    }
    if !bare.contains(' ') {
        if let Some(d) = super::threat::normalize(bare) {
            return Some(Peer::Fqdn(d));
        }
    }
    None
}

fn peer_label(p: &Peer) -> String {
    match p {
        Peer::Vms(m) => m
            .get(LABEL_VM_NAME)
            .cloned()
            .unwrap_or_else(|| m.values().cloned().collect::<Vec<_>>().join("-")),
        Peer::All => "all".into(),
        Peer::Entity(e) => (*e).into(),
        Peer::Cidr(c) => c.replace(['/', '.', ':'], "-"),
        Peer::Fqdn(d) => d.replace('.', "-"),
    }
}

fn peer_words(p: &Peer) -> String {
    match p {
        Peer::Vms(m) => match m.get(LABEL_VM_NAME) {
            Some(n) => format!("VM {n}"),
            None => format!(
                "VMs {}",
                m.iter()
                    .map(|(k, v)| format!("{k}={v}"))
                    .collect::<Vec<_>>()
                    .join(",")
            ),
        },
        Peer::All => "every VM".into(),
        Peer::Entity("host") => "the host".into(),
        Peer::Entity(_) => "the internet".into(),
        Peer::Cidr(c) => c.clone(),
        Peer::Fqdn(d) => d.clone(),
    }
}

fn selector(p: &Peer) -> Value {
    match p {
        Peer::Vms(m) => json!({ "matchLabels": m }),
        _ => json!({}),
    }
}

pub(super) fn slug(s: &str) -> String {
    let mut out = String::new();
    for c in s.to_ascii_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_matches('-').to_string();
    out.chars()
        .take(63)
        .collect::<String>()
        .trim_end_matches('-')
        .to_string()
}

fn ports_json(ports: &[Port]) -> Value {
    json!([{ "ports": ports.iter().map(|p| json!({ "port": p.port.to_string(), "protocol": p.proto })).collect::<Vec<_>>() }])
}

fn ports_words(ports: &[Port]) -> String {
    if ports.is_empty() {
        return "on every port".into();
    }
    format!(
        "on {}",
        ports
            .iter()
            .map(|p| format!("{}/{}", p.port, p.proto))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

/// One sentence → one policy and its description.
fn build(
    verb: Verb,
    from: Peer,
    to: Peer,
    ports: Vec<Port>,
    text: &str,
) -> Result<(VmNetworkPolicy, String), String> {
    let egress = matches!(to, Peer::Entity(_) | Peer::Cidr(_) | Peer::Fqdn(_));
    let (subject, other) = if egress { (&from, &to) } else { (&to, &from) };
    if !matches!(subject, Peer::Vms(_) | Peer::All) {
        return Err(format!(
            "{} cannot be the subject of a policy; name a VM or a label",
            peer_words(subject)
        ));
    }
    if !egress && matches!(other, Peer::Fqdn(_)) {
        return Err("a domain can only be a destination".into());
    }
    let mut rule = Map::new();
    let peer_key = match (egress, other) {
        (false, Peer::Vms(_) | Peer::All) => ("fromEndpoints", json!([selector(other)])),
        (false, Peer::Entity(e)) => ("fromEntities", json!([e])),
        (false, Peer::Cidr(c)) => ("fromCIDR", json!([c])),
        (true, Peer::Entity(e)) => ("toEntities", json!([e])),
        (true, Peer::Cidr(c)) => ("toCIDR", json!([c])),
        (true, Peer::Fqdn(d)) => ("toFQDNs", json!([{ "matchName": d }])),
        _ => return Err("unsupported combination".into()),
    };
    rule.insert(peer_key.0.into(), peer_key.1);
    if !ports.is_empty() {
        rule.insert("toPorts".into(), ports_json(&ports));
    }
    let dir = if egress { "egress" } else { "ingress" };
    let key = match verb {
        Verb::Deny => format!("{dir}Deny"),
        _ => dir.to_string(),
    };
    let mut rules = vec![Value::Object(rule)];
    let fqdn = matches!(to, Peer::Fqdn(_));
    if fqdn && verb != Verb::Deny {
        rules.push(json!({
            "toEntities": ["host"],
            "toPorts": [{
                "ports": [{ "port": "53", "protocol": "UDP" }, { "port": "53", "protocol": "TCP" }],
                "rules": { "dns": [{ "matchPattern": "*" }] },
            }],
        }));
    }
    let mut spec = Map::new();
    spec.insert("description".into(), json!(text));
    spec.insert("endpointSelector".into(), selector(subject));
    spec.insert(key, Value::Array(rules));
    if verb == Verb::Allow {
        let mut off = Map::new();
        off.insert(dir.into(), Value::Bool(false));
        spec.insert("enableDefaultDeny".into(), Value::Object(off));
    }
    let verb_word = match verb {
        Verb::Only => "only",
        Verb::Allow => "allow",
        Verb::Deny => "deny",
    };
    let name = slug(&format!(
        "nl-{verb_word}-{}-to-{}",
        peer_label(&from),
        peer_label(&to)
    ));
    let mut words = match verb {
        Verb::Only => format!(
            "{name}: {} accepts {dir} only from the listed peers — {} may reach {} {}; everything else {} is denied.",
            peer_words(subject),
            peer_words(&from),
            peer_words(&to),
            ports_words(&ports),
            if egress { "it sends" } else { "towards it" },
        ),
        Verb::Allow => format!(
            "{name}: {} may reach {} {} (adds an allow; does not isolate {}).",
            peer_words(&from),
            peer_words(&to),
            ports_words(&ports),
            peer_words(subject),
        ),
        Verb::Deny => format!(
            "{name}: {} may not reach {} {} (deny wins over any allow).",
            peer_words(&from),
            peer_words(&to),
            ports_words(&ports),
        ),
    };
    if fqdn && verb != Verb::Deny {
        words.push_str(" DNS to the host is allowed so the name can be resolved; add your resolver if it is elsewhere.");
    }
    Ok((
        VmNetworkPolicy {
            name,
            kind: "CiliumNetworkPolicy".into(),
            labels: [(LABEL_SOURCE.to_string(), SOURCE_VALUE.to_string())].into(),
            annotations: BTreeMap::new(),
            specs: vec![Value::Object(spec)],
        },
        words,
    ))
}

fn isolate(p: Peer, text: &str) -> Result<(VmNetworkPolicy, String), String> {
    if !matches!(p, Peer::Vms(_)) {
        return Err("isolate takes a VM or a label".into());
    }
    let name = slug(&format!("nl-isolate-{}", peer_label(&p)));
    let words = format!(
        "{name}: {} gets no traffic in or out (default deny both ways). For an incident, quarantine is faster and lifts itself.",
        peer_words(&p)
    );
    Ok((
        VmNetworkPolicy {
            name,
            kind: "CiliumNetworkPolicy".into(),
            labels: [(LABEL_SOURCE.to_string(), SOURCE_VALUE.to_string())].into(),
            annotations: BTreeMap::new(),
            specs: vec![json!({
                "description": text,
                "endpointSelector": selector(&p),
                "ingress": [{}],
                "egress": [{}],
            })],
        },
        words,
    ))
}

const REACH: &str = r"(?:reach|access|talk to|connect to|call|use|hit|send traffic to)";
const PORTS: &str = r"(?:\s+(?:on|over|via|at|using)\s+(?P<ports>.+?))?";

struct Patterns {
    can_only_be: Regex,
    can: Regex,
    allow_from: Regex,
    allow: Regex,
    must_not: Regex,
    deny_from: Regex,
    deny: Regex,
    isolate: Regex,
}

impl Patterns {
    fn new() -> Self {
        let r = |s: String| Regex::new(&s).unwrap();
        Self {
            can_only_be: r(format!(
                r"^(?P<to>.+?) (?:can|may|should) only be (?:reached|accessed|contacted) (?:by|from) (?P<from>.+?){PORTS}$"
            )),
            can: r(format!(
                r"^(?P<only>only )?(?P<from>.+?) (?:can|may|should be able to|is allowed to|are allowed to|should) (?P<only2>only )?{REACH} (?P<to>.+?){PORTS}$"
            )),
            allow_from: r(format!(
                r"^(?:allow|permit|let) (?:traffic |connections )?from (?P<from>.+?) to (?P<to>.+?){PORTS}$"
            )),
            allow: r(format!(
                r"^(?:allow|permit|let) (?P<from>.+?) (?:to )?{REACH} (?P<to>.+?){PORTS}$"
            )),
            must_not: r(format!(
                r"^(?P<from>.+?) (?:must not|mustn't|cannot|can't|may not|should not|shouldn't|must never|never) {REACH} (?P<to>.+?){PORTS}$"
            )),
            deny_from: r(format!(
                r"^(?:deny|block|forbid|drop) (?:traffic |connections )?from (?P<from>.+?) to (?P<to>.+?){PORTS}$"
            )),
            deny: r(format!(
                r"^(?:deny|block|forbid|prevent|stop) (?P<from>.+?) (?:from (?:reaching|accessing|talking to|connecting to)|(?:to )?{REACH}|from) (?P<to>.+?){PORTS}$"
            )),
            isolate: r(r"^(?:isolate|lock down|wall off|cut off) (?P<vm>.+)$".to_string()),
        }
    }
}

fn sentences(text: &str) -> Vec<String> {
    let space = Regex::new(r"\s+").unwrap();
    Regex::new(r"[\n;]+|\.\s+|\.$")
        .unwrap()
        .split(text)
        .map(|s| {
            space
                .replace_all(s.trim(), " ")
                .to_ascii_lowercase()
                .trim_end_matches(['.', '!'])
                .trim_start_matches("please ")
                .to_string()
        })
        .filter(|s| !s.is_empty())
        .collect()
}

/// Draft policies from plain English with the rule parser.
pub fn draft_rules(text: &str, inv: &[NetpolVm]) -> Draft {
    let pats = Patterns::new();
    let mut d = Draft {
        source: "rules".into(),
        ..Default::default()
    };
    for s in sentences(text) {
        let parsed = parse_sentence(&pats, &s, inv);
        match parsed {
            Ok((p, words)) => {
                if d.policies.iter().any(|x| x.name == p.name) {
                    continue;
                }
                d.policies.push(p);
                d.notes.push(words);
            }
            Err(why) => d.unparsed.push(format!("{s} — {why}")),
        }
    }
    d.yaml = to_yaml(&d.policies);
    d
}

fn parse_sentence(
    pats: &Patterns,
    s: &str,
    inv: &[NetpolVm],
) -> Result<(VmNetworkPolicy, String), String> {
    let get = |c: &regex::Captures, k: &str| c.name(k).map(|m| m.as_str().to_string());
    let ends = |c: &regex::Captures| -> Result<(Peer, Peer, Vec<Port>), String> {
        let f = get(c, "from").unwrap_or_default();
        let t = get(c, "to").unwrap_or_default();
        let from = peer(&f, inv).ok_or_else(|| unknown(&f))?;
        let to = peer(&t, inv).ok_or_else(|| unknown(&t))?;
        let ports = match get(c, "ports") {
            Some(p) => parse_ports(&p)?,
            None => Vec::new(),
        };
        Ok((from, to, ports))
    };
    if let Some(c) = pats.isolate.captures(s) {
        let v = get(&c, "vm").unwrap_or_default();
        return isolate(peer(&v, inv).ok_or_else(|| unknown(&v))?, s);
    }
    if let Some(c) = pats.can_only_be.captures(s) {
        let (f, t, p) = ends(&c)?;
        if !matches!(t, Peer::Vms(_) | Peer::All) {
            return Err(format!(
                "{} cannot be the subject of a policy; name a VM or a label",
                peer_words(&t)
            ));
        }
        return build(Verb::Only, f, t, p, s);
    }
    for re in [&pats.must_not, &pats.deny_from, &pats.deny] {
        if let Some(c) = re.captures(s) {
            let (f, t, p) = ends(&c)?;
            return build(Verb::Deny, f, t, p, s);
        }
    }
    if let Some(c) = pats.can.captures(s) {
        let only = c.name("only").is_some() || c.name("only2").is_some();
        let (f, t, p) = ends(&c)?;
        return build(if only { Verb::Only } else { Verb::Allow }, f, t, p, s);
    }
    for re in [&pats.allow_from, &pats.allow] {
        if let Some(c) = re.captures(s) {
            let (f, t, p) = ends(&c)?;
            return build(Verb::Allow, f, t, p, s);
        }
    }
    Err("not understood; try “only A can reach B on PORT”, “allow A to reach B on PORT”, “block A from reaching B” or “isolate A”".into())
}

fn unknown(s: &str) -> String {
    format!(
        "unknown endpoint `{}`: use a VM name, a key=value label, host, the internet, a CIDR or a domain",
        s.trim()
    )
}

pub fn to_yaml(policies: &[VmNetworkPolicy]) -> String {
    policies
        .iter()
        .map(VmNetworkPolicy::to_yaml)
        .collect::<Vec<_>>()
        .join("---\n")
}

/// System prompt for the LLM: the policy dialect plus the VMs and labels
/// it may select.
pub fn llm_system_prompt(vms: &[NetpolVm], existing: &[String]) -> String {
    let mut inv = String::new();
    for v in vms.iter().take(200) {
        let labels: Vec<String> = v
            .labels
            .iter()
            .filter(|(k, _)| !k.starts_with("machina.io/"))
            .map(|(k, x)| format!("{k}={x}"))
            .collect();
        inv.push_str(&format!("- {} {}\n", v.name, labels.join(",")));
    }
    if vms.len() > 200 {
        inv.push_str(&format!("- … and {} more\n", vms.len() - 200));
    }
    format!(
        "You write Cilium network policies for virtual machines. VMs are Cilium endpoints.\n\
Reply with YAML only: one or more documents separated by ---, each\n\
apiVersion: cilium.io/v2, kind: CiliumNetworkPolicy, metadata.name (lowercase, dashes, prefix nl-),\n\
and spec with description (the user's intent in one sentence), endpointSelector and rules.\n\
Rules you may use: ingress/egress with fromEndpoints/toEndpoints (matchLabels), fromEntities/toEntities\n\
(host, world, all), fromCIDR/toCIDR, toFQDNs (matchName/matchPattern), toPorts (ports: port as string,\n\
protocol TCP/UDP/SCTP/ANY; optional rules.http method/path), ingressDeny/egressDeny for explicit denies,\n\
enableDefaultDeny {{ingress: false}} for an additive allow that must not isolate the VM.\n\
An ingress or egress section isolates the selected VMs in that direction (everything not listed is denied),\n\
so only use it when the user asks for exclusive access (\"only\"). With toFQDNs, also allow DNS to the\n\
host on port 53 UDP/TCP with rules.dns matchPattern \"*\".\n\
Select one VM with matchLabels {{machina.io/vm-name: NAME}}; select groups by the labels below.\n\
Never invent VMs or labels. If the request is unclear or impossible, reply with a YAML comment line\n\
starting with '# cannot:' and the reason.\n\
VMs (name labels):\n{inv}Existing policies: {}\n",
        if existing.is_empty() {
            "none".to_string()
        } else {
            existing.join(", ")
        }
    )
}

/// Follow-up prompt asking the LLM to fix its draft.
pub fn llm_repair_prompt(request: &str, yaml: &str, errors: &[String]) -> String {
    format!(
        "Request: {request}\n\nYour YAML:\n{yaml}\n\nIt has these problems:\n- {}\n\nReply with the corrected YAML only.",
        errors.join("\n- ")
    )
}

/// The YAML inside an LLM reply: a fenced block if there is one, otherwise
/// the text from the first document line.
pub fn extract_yaml(reply: &str) -> String {
    let fence = Regex::new(r"(?s)```(?:ya?ml)?\s*\n(.*?)```").unwrap();
    let blocks: Vec<String> = fence
        .captures_iter(reply)
        .map(|c| c[1].trim().to_string())
        .collect();
    if !blocks.is_empty() {
        return blocks.join("\n---\n");
    }
    let lines: Vec<&str> = reply.lines().collect();
    let start = lines
        .iter()
        .position(|l| {
            let l = l.trim_start();
            l.starts_with("apiVersion:") || l.starts_with("kind:") || l.starts_with("---")
        })
        .unwrap_or(0);
    lines[start..].join("\n").trim().to_string()
}

/// Why the LLM declined (`# cannot: …`), if it did.
pub fn llm_declined(reply: &str) -> Option<String> {
    reply
        .lines()
        .find_map(|l| l.trim().strip_prefix("# cannot:"))
        .map(|s| s.trim().to_string())
}

/// Parse an LLM draft, label the policies, and say what is wrong with it.
pub fn accept_llm(yaml: &str, vms: &[NetpolVm]) -> Result<Draft, Vec<String>> {
    let (mut policies, v) = parse_documents(yaml);
    let mut errors: Vec<String> = v.errors.iter().map(|i| i.to_string()).collect();
    if policies.is_empty() && errors.is_empty() {
        errors.push("no policy documents".into());
    }
    for p in &policies {
        for name in p.specs.iter().flat_map(selected_vm_names) {
            if !vms.iter().any(|v| v.name == name) {
                errors.push(format!("{}: there is no VM named `{name}`", p.name));
            }
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    let mut notes = Vec::new();
    for p in &mut policies {
        p.labels
            .insert(LABEL_SOURCE.to_string(), SOURCE_VALUE.to_string());
        notes.push(match p.description() {
            Some(d) => format!("{}: {d}", p.name),
            None => p.name.clone(),
        });
    }
    Ok(Draft {
        yaml: to_yaml(&policies),
        policies,
        source: "llm".into(),
        notes,
        unparsed: Vec::new(),
    })
}

/// The policy description in a chat message that asks for one
/// (`/policy …`, `policy: …`, `netpol: …`, `draft a network policy: …`).
pub fn policy_request(message: &str) -> Option<&str> {
    let re = Regex::new(
        r"(?is)^\s*(?:/(?:netpol|policy)\s+|(?:netpol|policy|network policy|draft (?:a )?(?:network )?policy)\s*:\s*)(?P<rest>\S.*)$",
    )
    .unwrap();
    re.captures(message)
        .and_then(|c| c.name("rest"))
        .map(|m| m.as_str().trim())
}

/// A chat answer for a draft: what it does, the YAML, and how to apply it.
pub fn chat_reply(d: &Draft) -> String {
    let mut out = String::new();
    if d.policies.is_empty() {
        out.push_str("I could not turn that into a network policy.\n");
    } else {
        out.push_str(&format!(
            "Drafted {} VM network {} ({}). Nothing is applied yet.\n",
            d.policies.len(),
            if d.policies.len() == 1 {
                "policy"
            } else {
                "policies"
            },
            if d.source == "llm" {
                "Zyvor"
            } else {
                "sentence parser"
            }
        ));
    }
    for n in &d.notes {
        out.push_str(&format!("- {n}\n"));
    }
    for u in &d.unparsed {
        out.push_str(&format!("- not used: {u}\n"));
    }
    if !d.policies.is_empty() {
        out.push_str(&format!(
            "\n```yaml\n{}```\n\nTo apply it, open Network policies → Editor → Describe in plain English: \
             it replays recorded flows against the draft and sends it for approval.",
            d.yaml
        ));
    }
    out
}

fn selected_vm_names(spec: &Value) -> Vec<String> {
    let mut out = Vec::new();
    fn walk(v: &Value, out: &mut Vec<String>) {
        match v {
            Value::Object(m) => {
                for (k, x) in m {
                    if k == LABEL_VM_NAME {
                        if let Some(s) = x.as_str() {
                            out.push(s.to_string());
                        }
                    } else {
                        walk(x, out);
                    }
                }
            }
            Value::Array(a) => a.iter().for_each(|x| walk(x, out)),
            _ => {}
        }
    }
    walk(spec, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inv() -> Vec<NetpolVm> {
        let vm = |n: &str, app: &str| NetpolVm {
            name: n.into(),
            labels: [("app".to_string(), app.to_string())].into(),
            ..Default::default()
        };
        vec![vm("web-1", "web"), vm("web-2", "web"), vm("db-1", "db")]
    }

    fn spec(d: &Draft, i: usize) -> &Value {
        &d.policies[i].specs[0]
    }

    #[test]
    fn only_becomes_an_isolating_ingress_rule() {
        let d = draft_rules("Only web servers can reach the db on port 5432.", &inv());
        assert!(d.unparsed.is_empty(), "{:?}", d.unparsed);
        let s = spec(&d, 0);
        assert_eq!(s["endpointSelector"]["matchLabels"]["app"], "db");
        assert_eq!(
            s["ingress"][0]["fromEndpoints"][0]["matchLabels"]["app"],
            "web"
        );
        assert_eq!(s["ingress"][0]["toPorts"][0]["ports"][0]["port"], "5432");
        assert!(s.get("enableDefaultDeny").is_none());
        assert_eq!(d.policies[0].name, "nl-only-web-to-db");
        let (back, v) = parse_documents(&d.yaml);
        assert!(v.ok(), "{:?}\n{}", v.errors, d.yaml);
        assert_eq!(
            back[0].labels.get(LABEL_SOURCE).map(String::as_str),
            Some(SOURCE_VALUE)
        );
    }

    #[test]
    fn allow_is_additive_and_deny_and_egress_targets() {
        let d = draft_rules(
            "allow web-1 to reach db-1 on ssh and 8080/tcp\n\
             block db-1 from reaching the internet\n\
             web-2 can reach github.com on https\n\
             db-1 must not reach 10.0.0.0/8 on dns\n\
             isolate app=db",
            &inv(),
        );
        assert!(d.unparsed.is_empty(), "{:?}", d.unparsed);
        assert_eq!(d.policies.len(), 5);
        let a = spec(&d, 0);
        assert_eq!(a["endpointSelector"]["matchLabels"][LABEL_VM_NAME], "db-1");
        assert_eq!(a["enableDefaultDeny"]["ingress"], false);
        let ports: Vec<&str> = a["ingress"][0]["toPorts"][0]["ports"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["port"].as_str().unwrap())
            .collect();
        assert_eq!(ports, ["22", "8080"]);
        let b = spec(&d, 1);
        assert_eq!(b["egressDeny"][0]["toEntities"][0], "world");
        let c = spec(&d, 2);
        assert_eq!(c["egress"][0]["toFQDNs"][0]["matchName"], "github.com");
        assert_eq!(
            c["egress"][1]["toPorts"][0]["rules"]["dns"][0]["matchPattern"],
            "*"
        );
        let e = spec(&d, 3);
        assert_eq!(e["egressDeny"][0]["toCIDR"][0], "10.0.0.0/8");
        assert_eq!(
            e["egressDeny"][0]["toPorts"][0]["ports"][0]["protocol"],
            "UDP"
        );
        assert_eq!(spec(&d, 4)["ingress"], json!([{}]));
        let (_, v) = parse_documents(&d.yaml);
        assert!(v.ok(), "{:?}\n{}", v.errors, d.yaml);
    }

    #[test]
    fn unknown_things_are_reported_not_guessed() {
        let d = draft_rules("only payroll can reach db on 5432; make it secure", &inv());
        assert!(d.policies.is_empty());
        assert_eq!(d.unparsed.len(), 2);
        assert!(
            d.unparsed[0].contains("unknown endpoint `payroll`"),
            "{:?}",
            d.unparsed
        );
        let d = draft_rules("the host can only be reached by web-1", &inv());
        assert!(
            d.unparsed[0].contains("cannot be the subject"),
            "{:?}",
            d.unparsed
        );
    }

    #[test]
    fn chat_requests_need_an_explicit_prefix() {
        assert_eq!(policy_request("/policy isolate db-1"), Some("isolate db-1"));
        assert_eq!(
            policy_request("Policy: block db-1 from reaching the internet"),
            Some("block db-1 from reaching the internet")
        );
        assert_eq!(
            policy_request("draft a network policy: isolate web-1"),
            Some("isolate web-1")
        );
        assert_eq!(policy_request("what policy protects db-1?"), None);
        let d = draft_rules("isolate db-1", &inv());
        let r = chat_reply(&d);
        assert!(
            r.contains("Nothing is applied") && r.contains("```yaml"),
            "{r}"
        );
        assert!(chat_reply(&draft_rules("make it nice", &inv())).contains("could not"));
    }

    #[test]
    fn llm_replies_are_extracted_and_checked() {
        let reply = "Sure:\n```yaml\napiVersion: cilium.io/v2\nkind: CiliumNetworkPolicy\nmetadata:\n  name: nl-db\nspec:\n  description: db from web\n  endpointSelector:\n    matchLabels:\n      machina.io/vm-name: db-9\n  ingress:\n  - fromEndpoints:\n    - matchLabels:\n        app: web\n```\nDone.";
        let y = extract_yaml(reply);
        assert!(y.starts_with("apiVersion"));
        let errs = accept_llm(&y, &inv()).unwrap_err();
        assert!(errs[0].contains("no VM named `db-9`"), "{errs:?}");
        let d = accept_llm(&y.replace("db-9", "db-1"), &inv()).unwrap();
        assert_eq!(d.source, "llm");
        assert!(d.yaml.contains("machina.io/source: plain-english"));
        assert_eq!(
            llm_declined("# cannot: no such VM"),
            Some("no such VM".into())
        );
        assert!(llm_system_prompt(&inv(), &[]).contains("- web-1 app=web"));
    }
}
