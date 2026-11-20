#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

# VM network policy (CiliumNetworkPolicy schema), VM labels and packet-flow
# commands shared by machinactl (daemon, or the controller via --fleet) and
# platformctl (controller). Callers set NP_BASE (…/api/v1), NP_AUTH (curl
# args array) and NP_FLEET (1 when NP_BASE is the controller).

NP_BASE="${NP_BASE:-}"
NP_FLEET="${NP_FLEET:-0}"
if ! declare -p NP_AUTH &>/dev/null; then NP_AUTH=(); fi
NP_COLOR="${NP_COLOR:-auto}"

np_die() { echo "error: $*" >&2; exit 1; }

np_need() {
    command -v curl >/dev/null || np_die "curl is required"
    command -v jq >/dev/null || np_die "jq is required"
}

np_color_on() {
    case "$NP_COLOR" in
        always) return 0 ;;
        never) return 1 ;;
    esac
    [[ -t 1 && -z "${NO_COLOR:-}" && "${TERM:-}" != "dumb" ]]
}

# curl with auth; prints the body, fails with the server's error message on HTTP >= 400.
np_api() {
    local method=$1 path=$2
    shift 2
    local out code
    out=$(curl -sk -X "$method" "${NP_AUTH[@]}" -w $'\n%{http_code}' "$@" "${NP_BASE}${path}") || np_die "request to ${NP_BASE}${path} failed"
    code=${out##*$'\n'}
    out=${out%$'\n'*}
    if [[ "$code" -ge 400 || "$code" == "000" ]]; then
        local msg
        msg=$(jq -r '.error // .message // empty' <<<"$out" 2>/dev/null)
        echo "error: HTTP $code${msg:+: $msg}" >&2
        jq -r '(.errors // [])[] | "  \(.path): \(.message)"' <<<"$out" 2>/dev/null >&2
        exit 1
    fi
    printf '%s' "$out"
}

np_uri() { jq -rn --arg v "$1" '$v|@uri'; }

np_read_file() {
    local f=$1
    [[ -n "$f" ]] || np_die "missing -f FILE (use - for stdin)"
    if [[ "$f" == "-" ]]; then cat; else [[ -r "$f" ]] || np_die "cannot read $f"; cat "$f"; fi
}

np_print_issues() {
    jq -r '
      ((.errors // [])[] | "  \u001b[31merror\u001b[0m   \(.path): \(.message)"),
      ((.warnings // [])[] | "  \u001b[33mwarning\u001b[0m \(.path): \(.message)"),
      ((.compile_warnings // [])[] | "  \u001b[33mwarning\u001b[0m \(.)")' | np_strip
}

# Drop ANSI escapes when color is off.
np_strip() {
    if np_color_on; then cat; else sed $'s/\x1b\\[[0-9;]*m//g'; fi
}

# ── netpol ────────────────────────────────────────────────────────────────

np_netpol_usage() {
    cat <<'EOF'
Usage: netpol <command> [options]

  apply -f FILE|- [--dry-run]    Create/replace policies (CiliumNetworkPolicy /
                                 CiliumClusterwideNetworkPolicy / VmNetworkPolicy;
                                 multi-document YAML, JSON or kind: List)
  validate -f FILE|-             Schema check + preview of selected VMs and rules
  get [NAME] [-o yaml|json|wide] List policies, or show one
  delete NAME                    Delete a policy
  enable NAME | disable NAME     Toggle a policy (fleet only)
  test --from VM --to VM|IP|NAME [--port N] [--proto tcp|udp|sctp|icmp|any] [--icmp-type N]
       [--http-method M --path P --host H --header 'K: V'] [--sni NAME] [--dns-name NAME]
       [--kafka-api-key produce|fetch|… --kafka-topic T --kafka-client-id C --kafka-api-version N]
                                 Trace a connection (and an L7 request) through the
                                 policy set (like `cilium policy trace`)
  selectors                      Selector → matched VMs (like `cilium policy selectors`)
  endpoints                      VMs with identity, labels and enforcement
  status                         Sync state, enforcement mode, Cilium presence
  fqdn                           toFQDNs names learned from DNS replies to VMs
                                 (like `cilium fqdn cache list`)
  auth                           Mutual-authentication table (identity pairs,
                                 like `cilium-dbg auth list`)
  sync                           Push compiled policy to every host now (fleet only)
  learn [--vm VM] [--group-by LABEL] [--min N] [--no-l7] [--lock-unobserved] [-o yaml|json]
                                 Generate least-privilege policies from the 7-day
                                 flow history (prints YAML; review, then apply -f)
  replay -f FILE|-               What the YAML would have done to every connection
                                 in the flow history (would break / newly allow)
  draft "TEXT" [--rules] [--propose] [-o yaml|json]
                                 Plain English → policy YAML, validated and replayed
                                 against the flow history; nothing is applied
                                 (netpol draft help)
  projects                       Fleet Cloud projects: isolation, egress allowlist,
                                 egress IPs (fleet; netpol project help)
  project isolate|open|inherit|reset P | project default isolated|open
          | project assign P|- VM... | project approve|reject ID
                                 Default isolation between projects
  egress [P] [allow TO|remove TO|restrict|unrestrict|ip HOST IP|ip HOST -]
                                 Project egress allowlists and egress IPs
  overlay [enable|disable] [--prefix4 CIDR] [--prefix6 CIDR] [--port N]
                                 WireGuard overlay between hosts: fleet addresses,
                                 VM identity across NAT (fleet)
  evidence [-o summary|json|md] [--out FILE] | evidence verify FILE
                                 Segmentation evidence for audits: policies with
                                 hashes, project isolation, reachability matrix,
                                 denied flows, approvals; SHA-256 sealed
  quarantines                    Quarantined VMs, time left and exceptions
                                 (quarantine with `vm quarantine VM`)
  jit [list|grant|approve|reject|revoke]
                                 Temporary access that removes itself
                                 (netpol jit help)
  threat [list|set|refresh|rm]   DNS threat feeds: alert on (and optionally block)
                                 listed domains VMs resolve (netpol threat help)
EOF
}

np_evidence_digest() {
    jq -cj '.digest = "" | del(.signature)' "$1" | if command -v sha256sum >/dev/null; then sha256sum; else shasum -a 256; fi | cut -d' ' -f1
}

# Check the signature over the digest, and that the signer chains to
# the CA in the report (or to CA_FILE, the fleet CA the auditor trusts).
np_evidence_verify_signature() {
    local file=$1 digest=$2 trust=${3:-}
    if [[ "$(jq -r '.signature.value // empty' "$file")" == "" ]]; then
        echo "unsigned (host reports and older fleet reports carry only the digest)"
        return
    fi
    command -v openssl >/dev/null || np_die "openssl is needed to check the signature"
    local d
    d=$(mktemp -d) || np_die "mktemp failed"
    jq -r '.signature.signer_pem' "$file" >"$d/signer.pem"
    jq -r '.signature.ca_pem' "$file" >"$d/ca.pem"
    jq -r '.signature.value' "$file" | openssl base64 -d -A >"$d/sig.der"
    openssl x509 -in "$d/signer.pem" -pubkey -noout >"$d/pub.pem" 2>/dev/null
    local ca="$d/ca.pem"
    [[ -n "$trust" ]] && ca=$trust
    if ! printf '%s' "$digest" | openssl dgst -sha256 -verify "$d/pub.pem" -signature "$d/sig.der" >/dev/null 2>&1; then
        rm -rf "$d"
        echo "BAD SIGNATURE: the digest was not signed by the included certificate" >&2
        exit 1
    fi
    if ! openssl verify -CAfile "$ca" "$d/signer.pem" >/dev/null 2>&1; then
        rm -rf "$d"
        echo "UNTRUSTED: the signing certificate does not chain to ${trust:-the included CA}" >&2
        exit 1
    fi
    local fp
    fp=$(openssl x509 -in "$ca" -noout -fingerprint -sha256 | cut -d= -f2)
    rm -rf "$d"
    echo "ok: signed ($(jq -r .signature.alg "$file")) by a certificate of CA $fp"
    [[ -n "$trust" ]] || echo "  compare that fingerprint with your fleet CA, or pass it: netpol evidence verify FILE CA.pem" >&2
}

np_evidence_archive() {
    np_need_fleet
    local name="${1:-}" out=json file=""
    if [[ -z "$name" ]]; then
        local body
        body=$(np_api GET /vm-network-policies/evidence/archive)
        if [[ "${NP_JSON:-0}" == 1 ]]; then jq . <<<"$body"; return; fi
        if [[ "$(jq '.items | length' <<<"$body")" == 0 ]]; then
            jq -r '"(no stored reports in \(.dir))"' <<<"$body" >&2
            return
        fi
        { printf 'NAME\tBYTES\tMODIFIED\n'; jq -r '.items[] | [.name, (.bytes | tostring), .modified] | @tsv' <<<"$body"; } | column -t -s $'\t'
        return
    fi
    shift
    while [[ $# -gt 0 ]]; do
        case "$1" in
            -o|--output) out=$2; shift 2 ;;
            --out|--file) file=$2; shift 2 ;;
            *) np_die "unknown archive option: $1" ;;
        esac
    done
    local path
    path="/vm-network-policies/evidence/archive/$(np_uri "$name")"
    [[ "$out" == md || "$out" == markdown ]] && path="$path?format=md"
    if [[ -n "$file" ]]; then
        np_api GET "$path" >"$file" || np_die "cannot write $file"
        echo "wrote $file"
    else
        np_api GET "$path"
    fi
}

np_netpol_evidence() {
    local out="summary" file=""
    local probes="" project=""
    if [[ "${1:-}" == archive ]]; then
        shift
        np_evidence_archive "$@"
        return
    fi
    if [[ "${1:-}" == verify ]]; then
        file=${2:?usage: netpol evidence verify FILE [CA.pem]}
        [[ -r "$file" ]] || np_die "cannot read $file"
        local want got
        want=$(jq -r '.digest // empty' "$file") || np_die "$file is not JSON evidence"
        [[ -n "$want" ]] || np_die "$file has no digest"
        got=$(np_evidence_digest "$file")
        if [[ "$want" == "$got" ]]; then
            echo "ok: digest $want matches"
        else
            echo "MISMATCH: recorded $want, content hashes to $got" >&2
            exit 1
        fi
        np_evidence_verify_signature "$file" "$want" "${3:-}"
        return
    fi
    while [[ $# -gt 0 ]]; do
        case "$1" in
            -o|--output) out=$2; shift 2 ;;
            --out|--file) file=$2; shift 2 ;;
            --probes) probes=$2; shift 2 ;;
            --project) project=$2; shift 2 ;;
            -h|--help|help)
                echo 'Usage: netpol evidence [-o summary|json|md] [--out FILE] [--probes tcp/22,udp/53,...] [--project P]'
                echo '       netpol evidence verify FILE [CA.pem]     (digest, signature and CA chain)'
                echo '       netpol evidence archive [NAME [-o json|md] [--out FILE]]   (scheduled reports, fleet)'
                return ;;
            *) np_die "unknown evidence option: $1" ;;
        esac
    done
    local body q=""
    [[ -n "$probes" ]] && q="probes=$(np_uri "$probes")"
    [[ -n "$project" ]] && q="${q:+$q&}project=$(np_uri "$project")"
    case "$out" in
        md|markdown) body=$(np_api GET "/vm-network-policies/evidence?format=md${q:+&$q}") ;;
        json|summary) body=$(np_api GET "/vm-network-policies/evidence${q:+?$q}") ;;
        *) np_die "-o must be summary, json or md" ;;
    esac
    if [[ -n "$file" ]]; then
        printf '%s\n' "$body" >"$file" || np_die "cannot write $file"
        [[ "$out" == summary ]] || { echo "wrote $file"; return; }
    fi
    case "$out" in
        md|markdown|json) printf '%s\n' "$body" ;;
        summary)
            jq -r '
              "Segmentation evidence — \(.scope) (\(.source))",
              "generated \(.generated_at) by \(.generated_by)",
              "",
              "  VMs \(.summary.vms)   policies \(.summary.policies) (+\(.summary.generated_policies) generated)   covered by a policy \(.summary.vms_selected)",
              "  hosts \(.summary.hosts) (\(.summary.hosts_in_sync) in sync, \(.summary.hosts_enforcing) enforcing)   isolated projects \(.summary.projects_isolated)/\(.summary.projects)",
              "  denied connections \(.summary.denied_connections)   alerts \(.summary.alerts)   quarantines \(.quarantines | length)   approvals \(.approvals | length)",
              "",
              "  matrix (\(.matrix_groups)):",
              (.matrix[] | select(.from != .to or (.allowed | length) > 0) | "    \(.from) → \(.to): \(if (.allowed | length) == 0 then "deny" else (.allowed | join(", ")) end)"),
              "",
              (.warnings[]? | "  warning: \(.)"),
              "  sha256 \(.digest)\(if .signature then "  (signed)" else "" end)"' <<<"$body"
            [[ -n "$file" ]] && echo "wrote $file"
            ;;
    esac
}

np_netpol_learn() {
    local out="yaml" body req='{}'
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --vm) req=$(jq -c --arg v "$2" '. + {vm: $v}' <<<"$req"); shift 2 ;;
            --group-by) req=$(jq -c --arg v "$2" '. + {group_by: $v}' <<<"$req"); shift 2 ;;
            --min|--min-count) req=$(jq -c --argjson v "$2" '. + {min_count: $v}' <<<"$req"); shift 2 ;;
            --no-l7) req=$(jq -c '. + {l7: false}' <<<"$req"); shift ;;
            --lock-unobserved) req=$(jq -c '. + {lock_unobserved: true}' <<<"$req"); shift ;;
            -o|--output) out="$2"; shift 2 ;;
            *) np_die "unknown option: $1" ;;
        esac
    done
    body=$(np_api POST /vm-network-policies/learn -H 'Content-Type: application/json' -d "$req")
    if [[ "$out" == json ]]; then jq . <<<"$body"; return; fi
    jq -r '"# learned \(.policies | length) policies from \(.edges_used) flow edges (\(.edges_skipped) skipped)",
        (.notes[] | "# note: \(.)")' <<<"$body" >&2
    jq -r '.yaml' <<<"$body"
}

np_netpol_replay() {
    local file="" text body
    while [[ $# -gt 0 ]]; do
        case "$1" in
            -f|--filename) file="$2"; shift 2 ;;
            *) np_die "unknown option: $1" ;;
        esac
    done
    text=$(np_read_file "$file")
    body=$(np_api POST /vm-network-policies/replay -H 'Content-Type: application/json' -d "$(jq -n --arg y "$text" '{yaml: $y}')")
    if [[ "${NP_JSON:-0}" == 1 ]]; then jq . <<<"$body"; return; fi
    jq -r '
      def row(c; tone): "  \(tone)\(c.src) → \(c.dst) \(c.proto | ascii_downcase)/\(c.port)\u001b[0m\(if c.request then "  \u001b[35m\(c.request)\u001b[0m" else "" end)  \u001b[90m\(c.flows) flows, \(c.before) → \(c.after), last \(c.last_seen)\u001b[0m";
      "Replayed \(.evaluated) connections: \(.unchanged) unchanged, \(.would_break | length) would break, \(.would_allow | length) newly allowed\(if (.not_evaluated // 0) > 0 then ", \(.not_evaluated) skipped (endpoints no longer resolvable)" else "" end)",
      (if (.would_break | length) > 0 then "\n\u001b[1;31mWould break (\(.flows_breaking) flows)\u001b[0m", (.would_break[] | row(.; "\u001b[31m")) else empty end),
      (if (.would_allow | length) > 0 then "\n\u001b[1;33mWould newly allow\u001b[0m", (.would_allow[] | row(.; "\u001b[33m")) else empty end),
      (if (.would_break | length) == 0 then "\n\u001b[1;32mSafe: no observed connection would be blocked\u001b[0m" else empty end)' <<<"$body" | np_strip
    [[ "$(jq '.would_break | length' <<<"$body")" == 0 ]]
}

np_netpol_get() {
    local name="" out=""
    while [[ $# -gt 0 ]]; do
        case "$1" in
            -o|--output) out="$2"; shift 2 ;;
            -o*) out="${1#-o}"; shift ;;
            *) name="$1"; shift ;;
        esac
    done
    if [[ -n "$name" ]]; then
        case "$out" in
            json) np_api GET "/vm-network-policies/$(np_uri "$name")" | jq . ;;
            *) np_api GET "/vm-network-policies/$(np_uri "$name")?format=yaml"; echo ;;
        esac
        return
    fi
    local body
    body=$(np_api GET /vm-network-policies)
    case "$out" in
        json) jq . <<<"$body" ;;
        yaml) jq -r '.items[] | "---", .yaml' <<<"$body" ;;
        *)
            {
                printf 'NAME\tKIND\tENABLED\tSELECTED\tDESCRIPTION\n'
                jq -r --arg wide "$out" '.items[] | [
                    .name, .kind, (if .enabled == false then "no" else "yes" end),
                    ((.selected_vms // []) | if $wide == "wide" then join(",") else (length|tostring) end),
                    (.description // "" | .[0:60])
                  ] | @tsv' <<<"$body"
            } | column -t -s $'\t'
            jq -r '(.warnings // [])[] | "warning: \(.)"' <<<"$body" >&2
            ;;
    esac
}

np_netpol_apply() {
    local file="" dry=0 validate_only=${NP_VALIDATE:-0}
    while [[ $# -gt 0 ]]; do
        case "$1" in
            -f|--filename) file="$2"; shift 2 ;;
            --dry-run|--dry-run=*) dry=1; shift ;;
            *) np_die "unknown option: $1" ;;
        esac
    done
    local text body
    text=$(np_read_file "$file")
    if [[ "$validate_only" == 1 ]]; then
        body=$(curl -sk -X POST "${NP_AUTH[@]}" -H 'Content-Type: application/yaml' --data-binary @- \
            "${NP_BASE}/vm-network-policies/validate" <<<"$text") || np_die "request failed"
        dry=1
    elif [[ "$dry" == 1 ]]; then
        body=$(np_api POST "/vm-network-policies?dry_run=1" -H 'Content-Type: application/yaml' --data-binary @- <<<"$text")
    else
        body=$(np_api POST /vm-network-policies -H 'Content-Type: application/yaml' --data-binary @- <<<"$text")
    fi
    if [[ "$dry" == 1 ]]; then
        local valid
        valid=$(jq -r '.valid' <<<"$body")
        jq -r '.policies[]? | "policy/\(.name) (\(.kind)) selects \((.selected_vms // []) | length) VM(s): \((.selected_vms // []) | join(", "))"' <<<"$body"
        jq -r '"compiled rules: \(.rules // 0)"' <<<"$body"
        np_print_issues <<<"$body"
        if [[ "$valid" == "true" ]]; then echo "valid"; else echo "invalid" >&2; exit 1; fi
        return
    fi
    jq -r '.applied[]? | "vmnetworkpolicy/\(.) configured"' <<<"$body"
    np_print_issues <<<"$body"
    jq -r 'if .sync then (if .sync.skipped then "sync: skipped (\(.sync.skipped))" elif .sync.ok then "sync: ok — \(.sync.vms) VM(s), \(.sync.rules) rule(s), \(.sync.peers) peer(s)" else "sync: FAILED — \(.sync.error)" end) else empty end' <<<"$body"
}

np_netpol_test() {
    local from="" to="" port="" proto="tcp" icmp="" l7='{}'
    l7add() { l7=$(jq -c --arg k "$1" --arg v "$2" '. + {($k): $v}' <<<"$l7"); }
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --from|--src) from="$2"; shift 2 ;;
            --to|--dst) to="$2"; shift 2 ;;
            --port|--dport) port="$2"; shift 2 ;;
            --proto|--protocol) proto="$2"; shift 2 ;;
            --icmp-type) icmp="$2"; proto="icmp"; shift 2 ;;
            --http-method|--method) l7add http_method "$2"; shift 2 ;;
            --path|--http-path) l7add http_path "$2"; shift 2 ;;
            --host|--http-host) l7add http_host "$2"; shift 2 ;;
            --header|--http-header) l7=$(jq -c --arg v "$2" '.http_headers += [$v]' <<<"$l7"); shift 2 ;;
            --sni|--server-name) l7add server_name "$2"; shift 2 ;;
            --dns-name|--query) l7add dns_name "$2"; shift 2 ;;
            --kafka-api-key) l7add kafka_api_key "$2"; shift 2 ;;
            --kafka-topic) l7add kafka_topic "$2"; shift 2 ;;
            --kafka-client-id) l7add kafka_client_id "$2"; shift 2 ;;
            --kafka-api-version) l7=$(jq -c --argjson v "$2" '. + {kafka_api_version: $v}' <<<"$l7"); shift 2 ;;
            *) np_die "unknown option: $1" ;;
        esac
    done
    [[ -n "$from" && -n "$to" ]] || np_die "usage: netpol test --from VM|IP --to VM|IP [--port N] [--proto tcp]"
    local q body
    q=$(jq -n --arg f "$from" --arg t "$to" --arg p "$port" --arg pr "$proto" --arg i "$icmp" --argjson l7 "$l7" \
        '{from: $f, to: $t, protocol: $pr}
         + (if $p != "" then {port: ($p|tonumber)} else {} end)
         + (if $i != "" then {icmp_type: ($i|tonumber)} else {} end)
         + $l7')
    body=$(np_api POST /vm-network-policies/trace -H 'Content-Type: application/json' -d "$q")
    jq -r '
      def ep(e): "\(e.vm // e.input) \u001b[90m(\(e.kind), identity \(e.identity))\u001b[0m";
      def side(s; n): "\u001b[1m\(n)\u001b[0m  \(s.vm // "-")  \(
          if s.verdict == "allowed" then "\u001b[32mallowed\u001b[0m"
          elif s.verdict == "denied" or s.verdict == "default-deny" or s.verdict == "l7-denied" or s.verdict == "auth-failed" then "\u001b[31m\(s.verdict)\u001b[0m"
          else "\u001b[90m\(s.verdict)\u001b[0m" end)\(if s.enforced then "  \u001b[90m[default-deny]\u001b[0m" else "" end)",
        (if s.rule then "    ↳ \(s.rule)" else empty end),
        (if s.auth then "    \u001b[36m⚿ authentication: \(s.auth)\u001b[0m" else empty end),
        (if s.l7 then "    \u001b[35m◆ L7: \(s.l7)\u001b[0m" else empty end);
      "Tracing \(ep(.from)) → \(ep(.to))  \(.protocol | ascii_upcase)\(if .port > 0 then "/\(.port)" else "" end)",
      "",
      side(.egress; "Egress  at source      "),
      side(.ingress; "Ingress at destination "),
      "",
      (if .allowed then "Final verdict: \u001b[1;32m\(.summary)\u001b[0m"
       else "Final verdict: \u001b[1;31m\(.summary)\u001b[0m" end)' <<<"$body" | np_strip
}

np_netpol_selectors() {
    local body
    body=$(np_api GET /vm-network-policies/selectors)
    {
        printf 'POLICY\tPATH\tSELECTOR\tMATCHES\n'
        jq -r '.items[] | [.policy, .path, .selector, ((.vms // []) | if length == 0 then "(none)" else join(",") end)] | @tsv' <<<"$body"
    } | column -t -s $'\t'
}

np_netpol_endpoints() {
    local body
    body=$(np_api GET /vm-network-policies/endpoints)
    {
        printf 'VM\tIDENTITY\tHOST\tINGRESS\tEGRESS\tADDRESSES\tLABELS\n'
        jq -r '.items[] | [
            .name, (.identity|tostring), (.host // "-"),
            (if .ingress_enforced then "enforced" else "allow-all" end),
            (if .egress_enforced then "enforced" else "allow-all" end),
            ((.addresses // []) | join(",") | if . == "" then "-" else . end),
            ((.labels // {}) | to_entries | map("\(.key)=\(.value)") | join(","))
          ] | @tsv' <<<"$body"
    } | column -t -s $'\t'
}

np_netpol_status() {
    local body
    body=$(np_api GET /vm-network-policies/status)
    if [[ "${NP_JSON:-0}" == 1 ]]; then jq . <<<"$body"; return; fi
    jq -r '
      "Policies:     \(.policies // (.items|length? // 0))",
      (if .managed_by then "Managed by:   \(.managed_by)" else empty end),
      (if .enforcement then (.enforcement | if (.mode | type) == "object" then .mode else . end) as $e
        | "Enforcement:  \($e.mode // "observe")\(if $e.lease_remaining_secs then " (lease \($e.lease_remaining_secs)s)" else "" end)" else empty end),
      (if has("hosts") then empty elif .cilium then "Cilium:       present (\(.cilium)) — Cilium enforces its own endpoints; Machina enforces libvirt VMs" else "Cilium:       absent — Machina eBPF enforces natively" end),
      (if .last_sync then "Last sync:    \(.last_sync.at // "-")  \(if .last_sync.skipped then "skipped: \(.last_sync.skipped)" elif .last_sync.ok then "ok (\(.last_sync.vms) VMs, \(.last_sync.rules) rules, \(.last_sync.peers) peers)" else "FAILED: \(.last_sync.error)" end)" else empty end),
      (if .edge then "VM edge:      \(.edge.owner // "-") owner, \((.edge.taps // []) | length) tap(s), flow log \(if .edge.flow_log then "on" else "off" end)\(if (.edge.fqdn_rules // 0) > 0 then ", toFQDNs \(.edge.fqdn_rules) rule(s) / \(.edge.fqdn_cache // 0) learned address(es)" else "" end)\(if (.edge.l7_rules // 0) > 0 then ", L7 \(.edge.l7_rules) rule(s)" else "" end)\(if (.edge.auth_entries // 0) > 0 then ", \(.edge.auth_entries) authenticated pair(s)" else "" end)" else empty end),
      (if .edge.auth_cert then "Auth cert:    host \(.edge.auth_cert.host_id), expires \(.edge.auth_cert.not_after | todate)\(if .edge.auth_cert.listening then ", mTLS on :4250" else ", not listening" end)" else empty end),
      (if (.edge.proxy // "") != "" then "L7 proxy:     \(.edge.proxy)" else empty end),
      ((.hosts // [])[] | "  host \(.hostname // .host_id)  \(
          if .reachable == false then "\u001b[90munreachable\u001b[0m"
          elif .ok == false then "\u001b[31m\(.error // "error")\u001b[0m"
          elif .ok then "\u001b[32mok\u001b[0m" else "\u001b[90mnot synced\u001b[0m" end)  vms=\(.vms // 0) rules=\(.rules // 0) peers=\(.peers // 0) taps=\(.taps // 0) owner=\(.owner // "-")\(if .enforcing then " \u001b[1;31menforcing\u001b[0m" else " observe" end)\(if .cilium then " cilium=\(.cilium)" else "" end)  synced \(.synced_at // "-")")' <<<"$body" | np_strip
}

np_netpol_fqdn() {
    local body
    body=$(np_api GET /vm-network-policies/fqdn-cache)
    if [[ "${NP_JSON:-0}" == 1 ]]; then jq . <<<"$body"; return; fi
    {
        printf 'NAME\tADDRESS\tIDENTITY\tVM\tHOST\tTTL\tPATTERNS\n'
        jq -r '.items[] | [
            .name, .address, (if .identity == 0 then "-" else (.identity|tostring) end), (.vm // "-" | if . == "" then "-" else . end),
            (.hostname // "-"), "\(.expires_in_secs)s", ((.patterns // []) | join(","))
          ] | @tsv' <<<"$body"
    } | column -t -s $'\t'
}

np_netpol_auth() {
    local body
    body=$(np_api GET /vm-network-policies/auth)
    if [[ "${NP_JSON:-0}" == 1 ]]; then jq . <<<"$body"; return; fi
    {
        printf 'SUBJECT\tPEER\tMODE\tHOST\tEXPIRES\tSTATE\n'
        jq -r '.items[] | [
            "\(.subject) [\(.subject_identity)]", "\(.peer) [\(.peer_identity)]", .mode, (.hostname // "-"),
            (if .expires_in_secs > 0 then "\(.expires_in_secs)s" else "-" end), .state
          ] | @tsv' <<<"$body"
    } | column -t -s $'\t'
}

np_netpol_main() {
    np_need
    local sub="${1:-}"
    shift || true
    case "$sub" in
        apply|create|replace) np_netpol_apply "$@" ;;
        validate|lint) NP_VALIDATE=1 np_netpol_apply "$@" ;;
        get|ls|list) np_netpol_get "$@" ;;
        delete|rm) np_api DELETE "/vm-network-policies/$(np_uri "${1:?usage: netpol delete NAME}")" | jq -r '"vmnetworkpolicy/\(.deleted // .name) deleted"' ;;
        enable|disable)
            [[ "$NP_FLEET" == 1 ]] || np_die "enable/disable is a fleet (controller) feature; on a single host, delete and re-apply"
            local on=true; [[ "$sub" == disable ]] && on=false
            np_api PUT "/vm-network-policies/$(np_uri "${1:?usage: netpol $sub NAME}")/enabled" -H 'Content-Type: application/json' -d "{\"enabled\":$on}" >/dev/null
            echo "vmnetworkpolicy/$1 ${sub}d"
            ;;
        test|trace) np_netpol_test "$@" ;;
        selectors) np_netpol_selectors ;;
        endpoints|ep) np_netpol_endpoints ;;
        status) np_netpol_status ;;
        fqdn|fqdn-cache|dns) np_netpol_fqdn ;;
        auth) np_netpol_auth ;;
        quarantines|quarantine|q) np_netpol_quarantines ;;
        jit|access) np_netpol_jit "$@" ;;
        threat|threats|threat-feeds) np_netpol_threat "$@" ;;
        sync)
            [[ "$NP_FLEET" == 1 ]] || np_die "sync is a fleet (controller) feature; the daemon resyncs on every change and every 60s"
            np_api POST /vm-network-policies/sync | jq .
            ;;
        observe) np_flow_main observe "$@" ;;
        learn) np_netpol_learn "$@" ;;
        replay) np_netpol_replay "$@" ;;
        draft|nl) np_netpol_draft "$@" ;;
        projects) np_projects_list ;;
        project|tenant) np_netpol_project "$@" ;;
        egress) np_netpol_egress "$@" ;;
        overlay|wg) np_netpol_overlay "$@" ;;
        evidence|audit) np_netpol_evidence "$@" ;;
        ""|help|-h|--help) np_netpol_usage ;;
        *) np_die "unknown netpol command: $sub (try: netpol help)" ;;
    esac
}

# ── VM labels ─────────────────────────────────────────────────────────────

# Controller label routes take the VM id; accept a name and resolve it.
np_vm_ref() {
    local ref=$1
    if [[ "$NP_FLEET" != 1 || "$ref" =~ ^[0-9a-fA-F-]{36}$ ]]; then
        printf '%s' "$ref"
        return
    fi
    local id
    id=$(np_api GET /vms | jq -r --arg n "$ref" '(if type == "array" then . else (.items // .vms // []) end) | map(select(.name == $n)) | .[0].id // empty')
    [[ -n "$id" ]] || np_die "no VM named $ref"
    printf '%s' "$id"
}

np_label_main() {
    np_need
    local vm="${1:-}"
    [[ -n "$vm" ]] || { echo "usage: vm label VM [key=value ...] [key- ...]" >&2; exit 1; }
    shift
    local ref cur
    ref=$(np_vm_ref "$vm")
    cur=$(np_api GET "/vms/$(np_uri "$ref")/labels" | jq -c '.labels // {}')
    if [[ $# -eq 0 ]]; then
        jq -r 'to_entries[] | "\(.key)=\(.value)"' <<<"$cur"
        return
    fi
    local a
    for a in "$@"; do
        if [[ "$a" == *=* ]]; then
            cur=$(jq -c --arg k "${a%%=*}" --arg v "${a#*=}" '. + {($k): $v}' <<<"$cur")
        elif [[ "$a" == *- ]]; then
            cur=$(jq -c --arg k "${a%-}" 'del(.[$k])' <<<"$cur")
        else
            np_die "expected key=value or key-, got: $a"
        fi
    done
    np_api PUT "/vms/$(np_uri "$ref")/labels" -H 'Content-Type: application/json' -d "{\"labels\":$cur}" \
        | jq -r '.labels // {} | to_entries[] | "\(.key)=\(.value)"'
    echo "vm/$vm labeled" >&2
}

# ── VM quarantine ─────────────────────────────────────────────────────────

# 90s, 15m, 4h, 1d or plain seconds → seconds.
np_duration() {
    local d=$1
    case "$d" in
        *s) echo "${d%s}" ;;
        *m) echo $(( ${d%m} * 60 )) ;;
        *h) echo $(( ${d%h} * 3600 )) ;;
        *d) echo $(( ${d%d} * 86400 )) ;;
        *[!0-9]*|"") np_die "bad duration: $d (e.g. 15m, 1h, 24h)" ;;
        *) echo "$d" ;;
    esac
}

np_quarantine_usage() {
    cat <<'EOF'
Usage: vm quarantine VM [--for 1h] [--allow-host-ssh] [--allow DIR:PEER[:PROTO[/PORT]]] [--reason TEXT] [--host H]
       vm release VM [--host H]

Cuts every flow of the VM — open connections too, in observe mode too — for
--for (default 1h, max 24h). The kernel lifts it at the deadline.
  --allow ingress:host:tcp/22   an exception; PEER is host, world, any or a VM
  --allow-host-ssh              same as --allow ingress:host:tcp/22
  --host H                      fleet: only this host (default: every host, so
                                the quarantine follows the VM if it migrates)
EOF
}

np_quarantine_main() {
    np_need
    local vm="${1:-}"
    [[ -n "$vm" && "$vm" != -* ]] || { np_quarantine_usage >&2; exit 1; }
    shift
    local secs=3600 host="" req
    req=$(jq -nc --argjson s "$secs" '{secs: $s, allow: []}')
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --for|--duration) secs=$(np_duration "$2"); req=$(jq -c --argjson s "$secs" '.secs = $s' <<<"$req"); shift 2 ;;
            --allow-host-ssh) req=$(jq -c '.allow_host_ssh = true' <<<"$req"); shift ;;
            --allow)
                local dir peer rest proto="" port=0
                IFS=: read -r dir peer rest <<<"$2"
                [[ -n "$dir" && -n "$peer" ]] || np_die "--allow DIR:PEER[:PROTO[/PORT]], e.g. ingress:host:tcp/22"
                if [[ -n "$rest" ]]; then proto=${rest%%/*}; [[ "$rest" == */* ]] && port=${rest#*/}; fi
                req=$(jq -c --arg d "$dir" --arg p "$peer" --arg pr "$proto" --argjson po "$port" \
                    '.allow += [{direction: $d, peer: $p, proto: $pr, port: $po}]' <<<"$req")
                shift 2 ;;
            --reason) req=$(jq -c --arg r "$2" '.reason = $r' <<<"$req"); shift 2 ;;
            --host) host="$2"; shift 2 ;;
            -h|--help) np_quarantine_usage; return ;;
            *) np_die "unknown option: $1" ;;
        esac
    done
    local body
    body=$(np_api POST "/vms/$(np_uri "$vm")/quarantine${host:+?host=$(np_uri "$host")}" -H 'Content-Type: application/json' -d "$req")
    if [[ "${NP_JSON:-0}" == 1 ]]; then jq . <<<"$body"; return; fi
    jq -r --arg vm "$vm" '
        (if .hosts then .hosts else [.] end) as $h
        | "vm/\($vm) quarantined until \($h[0].until)",
          ($h[] | select((.taps // []) | length > 0) | "  on \(.hostname // "this host"): \(.taps | join(", "))"),
          (if ($h | map(.taps // [] | length) | add) == 0 then "  (not running — applies when it starts)" else empty end),
          ((.errors // [])[] | "  \(.hostname): \(.error)")' <<<"$body"
}

np_release_main() {
    np_need
    local vm="${1:-}" host=""
    [[ -n "$vm" && "$vm" != -* ]] || { np_quarantine_usage >&2; exit 1; }
    shift
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --host) host="$2"; shift 2 ;;
            *) np_die "unknown option: $1" ;;
        esac
    done
    local body
    body=$(np_api DELETE "/vms/$(np_uri "$vm")/quarantine${host:+?host=$(np_uri "$host")}")
    if [[ "$(jq -r '.released' <<<"$body")" == true ]]; then
        echo "vm/$vm released"
    else
        echo "vm/$vm was not quarantined"
    fi
}

# ── scale to zero (controller only) ────────────────────────────────────────

np_sleep_usage() {
    cat <<'EOF'
Usage: vm sleep VM            Managed-save the VM; traffic to its addresses wakes it
       vm wake VM             Restore a sleeping VM now
       vm sleep-policy VM [MINUTES|inherit|never]   Show or set auto-sleep after idle
       vm sleep-policy --project NAME [MINUTES|never|clear]   Project default
       vm sleeping            Sleeping VMs, RAM handed back, recent sleeps and wakes
EOF
}

np_sleep_main() {
    np_need
    [[ "$NP_FLEET" == 1 ]] || np_die "sleep/wake run on the controller (--fleet)"
    local action=$1 vm="${2:-}"
    [[ -n "$vm" && "$vm" != -* ]] || { np_sleep_usage >&2; exit 1; }
    local ref body
    ref=$(np_vm_ref "$vm")
    body=$(np_api POST "/vms/$(np_uri "$ref")/$action")
    if [[ "${NP_JSON:-0}" == 1 ]]; then jq . <<<"$body"; return; fi
    jq -r --arg vm "$vm" --arg a "$action" '"vm/\($vm) \($a) queued (task \(.task_id))"' <<<"$body"
}

np_sleep_policy_main() {
    np_need
    [[ "$NP_FLEET" == 1 ]] || np_die "sleep policies live on the controller (--fleet)"
    if [[ "${1:-}" == --project ]]; then
        local project="${2:-}" val="${3:-}"
        [[ -n "$project" ]] || { np_sleep_usage >&2; exit 1; }
        case "$val" in
            "") np_api GET /sleep/policies | jq -r --arg p "$project" \
                    'map(select(.project == $p)) | if length == 0 then "project/\($p): no default (VMs never auto-sleep unless set)" else .[0] | "project/\(.project): \(if .sleep_after_minutes == 0 then "never" else "\(.sleep_after_minutes) min" end)" end' ;;
            clear) np_api DELETE "/sleep/policies/$(np_uri "$project")" >/dev/null; echo "project/$project default cleared" ;;
            *)
                [[ "$val" == never ]] && val=0
                [[ "$val" =~ ^[0-9]+$ ]] || np_die "MINUTES must be a number, never or clear"
                np_api PUT "/sleep/policies/$(np_uri "$project")" -H 'Content-Type: application/json' \
                    -d "{\"sleep_after_minutes\":$val}" >/dev/null
                echo "project/$project: VMs sleep after ${val} min idle (0 = never)" ;;
        esac
        return
    fi
    local vm="${1:-}" val="${2:-}"
    [[ -n "$vm" && "$vm" != -* ]] || { np_sleep_usage >&2; exit 1; }
    local ref body
    ref=$(np_vm_ref "$vm")
    if [[ -n "$val" ]]; then
        local m
        case "$val" in
            inherit) m=null ;;
            never) m=0 ;;
            *) [[ "$val" =~ ^[0-9]+$ ]] || np_die "MINUTES must be a number, inherit or never"; m=$val ;;
        esac
        body=$(np_api PUT "/vms/$(np_uri "$ref")/sleep-policy" -H 'Content-Type: application/json' -d "{\"sleep_after_minutes\":$m}")
    else
        body=$(np_api GET "/vms/$(np_uri "$ref")/sleep-policy")
    fi
    if [[ "${NP_JSON:-0}" == 1 ]]; then jq . <<<"$body"; return; fi
    jq -r --arg vm "$vm" '
        "vm/\($vm): \(if .desired_state == "sleeping" then "sleeping since \(.slept_at // "?")" else .observed_state end)",
        "  auto-sleep: \(if .effective_minutes == 0 then "off" else "after \(.effective_minutes) min idle" end)\(if .sleep_after_minutes == null then " (project default)" else "" end)",
        (if .idle_minutes != null and .desired_state != "sleeping" then "  idle:       \(.idle_minutes) min" else empty end),
        (if .wakeable then empty else "  no guest address known: traffic cannot wake it yet" end),
        (.events[:5][] | "  \(.at)  \(.kind) (\(.reason))")' <<<"$body"
}

np_sleeping_main() {
    np_need
    [[ "$NP_FLEET" == 1 ]] || np_die "sleep state lives on the controller (--fleet)"
    local body
    body=$(np_api GET /sleep/summary)
    if [[ "${NP_JSON:-0}" == 1 ]]; then jq . <<<"$body"; return; fi
    jq -r '"\(.sleeping | length) sleeping, \(.memory_freed_mib) MiB RAM and \(.vcpus_freed) vCPU handed back; \(.auto_sleep_vms) VMs with auto-sleep; last 24h: \(.sleeps_24h) sleeps, \(.wakes_24h) wakes"' <<<"$body"
    [[ "$(jq '.sleeping | length' <<<"$body")" != 0 ]] || return 0
    {
        printf 'VM\tPROJECT\tRAM\tSINCE\n'
        jq -r '.sleeping[] | [.name, (.project // "-"), "\(.memory_mib) MiB", (.slept_at // "-")] | @tsv' <<<"$body"
    } | column -t -s $'\t'
}

np_tt_usage() {
    cat <<'EOF'
Usage: vm restore-points VM [--every MIN|off] [--keep N]   List restore points, or schedule them
       vm restore-point VM [--note TEXT] [--wait]          Take a restore point now
       vm rewind VM POINT [--wait]                         Discard everything after POINT
       vm fork VM NEW [--at POINT] [--memory] [--isolate] [--no-reseed] [--stopped] [--wait]
       vm fork-detach VM [--wait]                          Copy the source's layers into the fork
POINT is a number from `vm restore-points` (1 = oldest, -1 = newest), a label or an id.
Without --at, fork copies the VM as it is now; --memory also copies its RAM onto an
isolated network.
EOF
}

# Waits for a controller task; fails with its message if it failed.
np_task_wait() {
    local id=$1 body st=""
    for _ in $(seq 600); do
        body=$(np_api GET "/tasks/$(np_uri "$id")")
        st=$(jq -r '.status' <<<"$body")
        case "$st" in
            completed) return 0 ;;
            failed|cancelled) np_die "task $id $st: $(jq -r '.message // .error // ""' <<<"$body")" ;;
        esac
        sleep 1
    done
    np_die "task $id still ${st:-unknown} after 600 s"
}

np_tt_queued() {
    local body=$1 what=$2 wait=$3
    if [[ "$wait" == 1 ]]; then
        np_task_wait "$(jq -r .task_id <<<"$body")"
        echo "$what done"
    elif [[ "${NP_JSON:-0}" == 1 ]]; then
        jq . <<<"$body"
    else
        echo "$what queued (task $(jq -r .task_id <<<"$body"))"
    fi
}

np_tt_point_id() {
    local ref=$1 sel=$2 body id
    body=$(np_api GET "/vms/$(np_uri "$ref")/restore-points")
    if [[ "$sel" =~ ^-?[0-9]+$ ]]; then
        id=$(jq -r --argjson i "$sel" '.points | if $i < 0 then .[length + $i] else .[$i - 1] end | .id // empty' <<<"$body")
    else
        id=$(jq -r --arg s "$sel" '.points[] | select(.id == $s or .label == $s) | .id' <<<"$body" | head -1)
    fi
    [[ -n "$id" ]] || np_die "no restore point $sel"
    printf '%s' "$id"
}

np_tt_main() {
    np_need
    [[ "$NP_FLEET" == 1 ]] || np_die "restore points and forks run on the controller (--fleet)"
    local action=$1
    shift
    local vm="${1:-}"
    [[ -n "$vm" && "$vm" != -* ]] || { np_tt_usage >&2; exit 1; }
    shift
    local ref body wait=0
    ref=$(np_vm_ref "$vm")
    case "$action" in
        list)
            local every="" keep=""
            while [[ $# -gt 0 ]]; do
                case "$1" in
                    --every) every=$2; shift 2 ;;
                    --keep) keep=$2; shift 2 ;;
                    *) np_tt_usage >&2; exit 1 ;;
                esac
            done
            if [[ -n "$every$keep" ]]; then
                [[ "$every" == off ]] && every=0
                [[ -n "$every" ]] || every=$(np_api GET "/vms/$(np_uri "$ref")/restore-points" | jq -r '.every_minutes // 0')
                [[ "$every" =~ ^[0-9]+$ ]] || np_die "--every takes minutes or off"
                [[ -z "$keep" || "$keep" =~ ^[0-9]+$ ]] || np_die "--keep takes a number"
                body=$(np_api PUT "/vms/$(np_uri "$ref")/restore-points/policy" -H 'Content-Type: application/json' \
                    -d "$(jq -nc --argjson e "$every" --arg k "$keep" '{every_minutes: $e} + (if $k == "" then {} else {keep: ($k | tonumber)} end)')")
            else
                body=$(np_api GET "/vms/$(np_uri "$ref")/restore-points")
            fi
            if [[ "${NP_JSON:-0}" == 1 ]]; then jq . <<<"$body"; return; fi
            jq -r --arg vm "$vm" '
                "vm/\($vm): \(.points | length) restore points; schedule \(if (.every_minutes // 0) == 0 then "off" else "every \(.every_minutes) min, keep \(.keep)" end)",
                (if .fork_of then "  fork of \(.fork_of.name // .fork_of.vm_id)\(if .fork_of.memory then " (with memory)" else "" end)" else empty end),
                (.points | to_entries[] | "  \(.key + 1)  \(.value.created_at)  \(.value.kind)  \(.value.label)\(if .value.quiesced then "  quiesced" else "" end)\(if (.value.forks | length) > 0 then "  forks: \(.value.forks | join(","))" else "" end)\(if .value.note then "  \(.value.note)" else "" end)"),
                (if (.forks | length) > 0 then "  forks: \([.forks[] | .name // .vm_id] | join(", "))" else empty end)' <<<"$body"
            ;;
        create)
            local note=""
            while [[ $# -gt 0 ]]; do
                case "$1" in
                    --note) note=$2; shift 2 ;;
                    --wait) wait=1; shift ;;
                    *) np_tt_usage >&2; exit 1 ;;
                esac
            done
            body=$(np_api POST "/vms/$(np_uri "$ref")/restore-points" -H 'Content-Type: application/json' \
                -d "$(jq -nc --arg n "$note" '{note: (if $n == "" then null else $n end)}')")
            np_tt_queued "$body" "vm/$vm restore point" "$wait"
            ;;
        rewind)
            local sel="${1:-}" pid
            [[ -n "$sel" ]] || { np_tt_usage >&2; exit 1; }
            shift
            [[ "${1:-}" == --wait ]] && wait=1
            pid=$(np_tt_point_id "$ref" "$sel")
            body=$(np_api POST "/vms/$(np_uri "$ref")/restore-points/$(np_uri "$pid")/rewind")
            np_tt_queued "$body" "vm/$vm rewind" "$wait"
            ;;
        fork)
            local new="${1:-}" at="" memory=false isolate=false reseed=true start=true
            [[ -n "$new" && "$new" != -* ]] || { np_tt_usage >&2; exit 1; }
            shift
            while [[ $# -gt 0 ]]; do
                case "$1" in
                    --at) at=$(np_tt_point_id "$ref" "$2"); shift 2 ;;
                    --memory) memory=true; shift ;;
                    --isolate) isolate=true; shift ;;
                    --no-reseed) reseed=false; shift ;;
                    --stopped) start=false; shift ;;
                    --wait) wait=1; shift ;;
                    *) np_tt_usage >&2; exit 1 ;;
                esac
            done
            body=$(np_api POST "/vms/$(np_uri "$ref")/fork" -H 'Content-Type: application/json' \
                -d "$(jq -nc --arg n "$new" --arg at "$at" --argjson m "$memory" --argjson i "$isolate" \
                    --argjson r "$reseed" --argjson s "$start" \
                    '{name: $n, memory: $m, isolate: $i, reseed: $r, start: $s} + (if $at == "" then {} else {restore_point_id: $at} end)')")
            np_tt_queued "$body" "vm/$vm fork $new" "$wait"
            ;;
        detach)
            [[ "${1:-}" == --wait ]] && wait=1
            body=$(np_api POST "/vms/$(np_uri "$ref")/fork/detach")
            np_tt_queued "$body" "vm/$vm detach" "$wait"
            ;;
    esac
}

np_netpol_quarantines() {
    local body
    body=$(np_api GET /vm-network-policies/quarantines)
    if [[ "${NP_JSON:-0}" == 1 ]]; then jq . <<<"$body"; return; fi
    [[ "$(jq '.items | length' <<<"$body")" != 0 ]] || { echo "(no VM is quarantined)" >&2; return; }
    {
        printf 'VM\tHOST\tLEFT\tUNTIL\tTAPS\tEXCEPTIONS\tREASON\tBY\n'
        jq -r '.items[] | [
            .vm, (.hostname // "-"), "\(.remaining_secs)s", .until, ((.taps // []) | join(",") | if . == "" then "-" else . end),
            ((.allow // []) | map("\(.direction) \(.peer) \(if .proto == "" then "any" else .proto end)\(if .port > 0 then "/\(.port)" else "" end)") | join("; ") | if . == "" then "-" else . end),
            (.reason // "" | if . == "" then "-" else . end), (.by // "" | if . == "" then "-" else . end)
          ] | @tsv' <<<"$body"
    } | column -t -s $'\t'
}

# ── just-in-time access ───────────────────────────────────────────────────

np_jit_usage() {
    cat <<'EOF'
Usage: netpol jit [list]
       netpol jit grant --from VM|host --to VM [--port N] [--proto tcp|udp|sctp|any] [--for 1h] [--reason TEXT] [--now]
       netpol jit approve ID | reject ID      (fleet)
       netpol jit revoke NAME

Temporary access: a policy that lets --from reach --to (one port, or every
port without --port) and is removed when --for runs out (default 1h, max
24h). It only adds an allow; it never isolates either VM.
On a single host, admins grant directly. With --fleet, grant files a
request in Approvals that another admin approves; --now (admins) skips it.
EOF
}

np_jit_list() {
    local body
    body=$(np_api GET /vm-network-policies/jit)
    if [[ "${NP_JSON:-0}" == 1 ]]; then jq . <<<"$body"; return; fi
    if [[ "$(jq '.items | length' <<<"$body")" == 0 ]]; then
        echo "(no temporary access)" >&2
    else
        {
            printf 'NAME\tFROM\tTO\tPORT\tLEFT\tUNTIL\tBY\tREASON\n'
            jq -r '.items[] | [
                .name, .from, .to, (if .port == 0 then "any" else "\(.port)/\(.protocol | ascii_downcase)" end),
                "\(.remaining_secs)s", .expires_at, (.granted_by // "-"), (.reason // "" | if . == "" then "-" else . end)
              ] | @tsv' <<<"$body"
        } | column -t -s $'\t'
    fi
    if [[ "$(jq '(.pending // []) | length' <<<"$body")" != 0 ]]; then
        echo
        {
            printf 'PENDING\tREQUEST\tBY\tSINCE\n'
            jq -r '.pending[] | [.id, .label, .requested_by, .created_at] | @tsv' <<<"$body"
        } | column -t -s $'\t'
    fi
}

np_jit_grant() {
    local req='{}' now=false
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --from) req=$(jq -c --arg v "$2" '.from = $v' <<<"$req"); shift 2 ;;
            --to) req=$(jq -c --arg v "$2" '.to = $v' <<<"$req"); shift 2 ;;
            --port) req=$(jq -c --argjson v "$2" '.port = $v' <<<"$req"); shift 2 ;;
            --proto|--protocol) req=$(jq -c --arg v "$2" '.protocol = ($v | ascii_upcase)' <<<"$req"); shift 2 ;;
            --for|--duration) req=$(jq -c --argjson v "$(np_duration "$2")" '.secs = $v' <<<"$req"); shift 2 ;;
            --reason) req=$(jq -c --arg v "$2" '.reason = $v' <<<"$req"); shift 2 ;;
            --now) now=true; shift ;;
            -h|--help) np_jit_usage; return ;;
            *) np_die "unknown option: $1" ;;
        esac
    done
    [[ "$(jq -r '.from // "" | length > 0' <<<"$req")" == true && "$(jq -r '.to // "" | length > 0' <<<"$req")" == true ]] \
        || { np_jit_usage >&2; exit 1; }
    [[ "$NP_FLEET" == 1 ]] && req=$(jq -c --argjson g "$now" '.grant = $g' <<<"$req")
    local body
    body=$(np_api POST /vm-network-policies/jit -H 'Content-Type: application/json' -d "$req")
    if [[ "${NP_JSON:-0}" == 1 ]]; then jq . <<<"$body"; return; fi
    jq -r '
      if .pending then "requested: \(.pending.label)\n  waiting for another admin: netpol jit approve \(.pending.id)  (or Approvals in the UI)"
      else "granted: \(.granted.from) → \(.granted.to)\(if .granted.port > 0 then ":\(.granted.port)/\(.granted.protocol | ascii_downcase)" else "" end) until \(.granted.expires_at)\n  policy \(.policy) (revoke early: netpol jit revoke \(.policy))" end' <<<"$body"
}

np_netpol_jit() {
    local sub="${1:-list}"
    shift || true
    case "$sub" in
        list|ls) np_jit_list ;;
        grant|request) np_jit_grant "$@" ;;
        approve|reject)
            [[ "$NP_FLEET" == 1 ]] || np_die "approvals are a fleet (controller) feature; on a single host admins grant directly"
            local id="${1:?usage: netpol jit $sub ID}" verb=execute done=approved
            [[ "$sub" == reject ]] && verb=reject done=rejected
            np_api POST "/ai/actions/$(np_uri "$id")/$verb" -H 'Content-Type: application/json' -d '{}' >/dev/null
            echo "request $id $done"
            ;;
        revoke|delete|rm)
            np_api DELETE "/vm-network-policies/$(np_uri "${1:?usage: netpol jit revoke NAME}")" >/dev/null
            echo "temporary access $1 revoked"
            ;;
        help|-h|--help) np_jit_usage ;;
        *) np_die "unknown jit command: $sub (try: netpol jit help)" ;;
    esac
}

# ── plain-English drafts ──────────────────────────────────────────────────

np_draft_usage() {
    cat <<'EOF'
Usage: netpol draft "TEXT" [--rules] [--propose] [-o yaml|json]
       netpol draft pending | approve ID | reject ID      (fleet)

Turns sentences into policy YAML and replays it against the flow history.
Nothing is applied: the YAML goes to stdout, notes and the replay summary
to stderr. Understood sentences:
  only web servers can reach the db on port 5432
  allow web-1 to reach db-1 on ssh and 8080/tcp
  block db-1 from reaching the internet
  web-2 can reach github.com on https
  db-1 must not reach 10.0.0.0/8 on dns
  isolate app=db
Endpoints are VM names, key=value labels, label values (web servers →
app=web), host, the internet, CIDRs and domains. With --fleet, Zyvor's LLM
drafts when one is configured (--rules skips it), and --propose files the
draft in Approvals for another admin. On a single host, review the YAML and
run `netpol apply -f`.
EOF
}

np_netpol_draft() {
    case "${1:-}" in
        ""|help|-h|--help) np_draft_usage; return ;;
        pending|approve|reject)
            [[ "$NP_FLEET" == 1 ]] || np_die "approvals are a fleet (controller) feature; on a single host run netpol apply -f"
            ;;
    esac
    case "${1:-}" in
        pending)
            local body
            body=$(np_api GET /vm-network-policies/draft)
            if [[ "${NP_JSON:-0}" == 1 ]]; then jq . <<<"$body"; return; fi
            if [[ "$(jq '.pending | length' <<<"$body")" == 0 ]]; then echo "(no drafts waiting for approval)" >&2; return; fi
            {
                printf 'ID\tREQUEST\tBY\tSINCE\n'
                jq -r '.pending[] | [.id, .label, .requested_by, .created_at] | @tsv' <<<"$body"
            } | column -t -s $'\t'
            return
            ;;
        approve|reject)
            local sub="$1" id="${2:?usage: netpol draft $1 ID}" verb=execute done=approved
            [[ "$sub" == reject ]] && verb=reject done=rejected
            np_api POST "/ai/actions/$(np_uri "$id")/$verb" -H 'Content-Type: application/json' -d '{}' >/dev/null
            echo "draft $id $done"
            return
            ;;
    esac
    local words=() out=yaml rules=false propose=false
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --rules) rules=true; shift ;;
            --propose) propose=true; shift ;;
            -o|--output) out="$2"; shift 2 ;;
            -*) np_die "unknown option: $1" ;;
            *) words+=("$1"); shift ;;
        esac
    done
    [[ ${#words[@]} -gt 0 ]] || { np_draft_usage >&2; exit 1; }
    if $propose && [[ "$NP_FLEET" != 1 ]]; then
        np_die "--propose needs --fleet; on a single host review the YAML and run netpol apply -f"
    fi
    local prompt="${words[*]}" body
    body=$(np_api POST /vm-network-policies/draft -H 'Content-Type: application/json' \
        -d "$(jq -n --arg p "$prompt" --argjson r "$rules" '{prompt: $p, rules_only: $r}')")
    if [[ "$out" == json ]]; then jq . <<<"$body"; else
        jq -r '
          "# drafted by \(if .source == "llm" then "Zyvor" else "the sentence parser" end)",
          (.notes[] | "# \(.)"),
          (.unparsed[] | "# not used: \(.)"),
          ((.preview.errors // [])[] | "# error: \(.message // .)"),
          ((.preview.warnings // [])[] | "# warning: \(.message // .)"),
          (.replay | "# replay: \(.evaluated) connections, \(.would_break | length) would break, \(.would_allow | length) newly allowed"),
          (.replay.would_break[:10][] | "#   would break: \(.src) → \(.dst) \(.proto | ascii_downcase)/\(.port) (\(.flows) flows)")' <<<"$body" >&2
        jq -r '.yaml' <<<"$body"
    fi
    if $propose; then
        local pend
        pend=$(np_api POST /vm-network-policies/draft/propose -H 'Content-Type: application/json' \
            -d "$(jq -n --arg y "$(jq -r .yaml <<<"$body")" --arg p "$prompt" '{yaml: $y, prompt: $p}')")
        jq -r '"requested: \(.pending.label)\n  waiting for another admin: netpol draft approve \(.pending.id)  (or Approvals in the UI)"' <<<"$pend" >&2
    fi
}

# ── Fleet Cloud project networking ────────────────────────────────────────

np_project_usage() {
    cat <<'EOF'
Usage: netpol projects
       netpol project isolate P [--no-host] | open P | inherit P | reset P
       netpol project default isolated|open [--no-host]
       netpol project assign P|- VM...                (put VMs in a project, - clears)
       netpol project approve ID | reject ID          (a proposed change; approve needs another admin)
       netpol egress                                  (egress IPs on every host)
       netpol egress P                                (one project)
       netpol egress P allow TO [--port N[/udp]]...   (and limit egress to the list)
       netpol egress P remove TO
       netpol egress P restrict | unrestrict
       netpol egress P ip HOST IP[,IPv6] | ip HOST -  (one IPv4, one IPv6, or both)
       netpol egress P require-ip | allow-host-ip     (block or allow egress on hosts without one)

Add --preview to any change to replay it against the flow history without
applying it, or --propose to ask a second admin (Approvals; also forced
for every change by MACHINA_NETPOL_PROJECT_APPROVAL=1 on the controller).
A project's own admins (Fleet Cloud project role) may change its isolation
and egress allowlist; egress IPs and the default need a fleet admin.

Fleet only (--fleet). An isolated project's VMs accept connections only
from VMs of the same project (and the host, unless --no-host). Projects
follow the default unless set; `inherit` returns to it, `reset` drops every
setting of the project. Ordinary allow policies still open exceptions.

A restricted project reaches only its allowlist (CIDRs, IPs, domains,
*.domain or world, optionally per port), its own VMs, the host and DNS.
Drops need the enforcement lease, like every policy.

An egress IP rewrites the source of project traffic leaving HOST (name or
id) to IP, which must already be configured on that host. Private,
link-local, CGNAT and multicast destinations are never rewritten. A VM
of the project on a host without one of its egress IPs leaves with the
host's address and raises an event; `require-ip` blocks its internet
egress there instead.
EOF
}

np_need_fleet() {
    [[ "$NP_FLEET" == 1 ]] || np_die "project networking is a fleet feature: add --fleet"
}

# np_project_settings P: the project's settings object (defaults if unset).
np_project_settings() {
    np_api GET /vm-network-policies/projects | jq -c --arg p "$1" '
      if $p == "*" then .default
      else ((.items[] | select(.project == $p) | .settings) // {project: $p, isolation: "inherit", allow_host: true, egress_restricted: false, egress_allow: [], egress_ips: {}})
      end'
}

# --preview / --dry-run (replay only) and --propose (ask a second admin),
# anywhere in a project or egress command; the rest lands in NP_ARGS.
np_project_mode() {
    NP_PMODE=apply
    NP_ARGS=()
    local a
    for a in "$@"; do
        case "$a" in
            --preview|--dry-run) NP_PMODE=preview ;;
            --propose) NP_PMODE=propose ;;
            *) NP_ARGS+=("$a") ;;
        esac
    done
}

np_project_preview_print() {
    if [[ "${NP_JSON:-0}" == 1 ]]; then jq . <<<"$1"; return; fi
    jq -r '"preview \(if .project == "*" then "default" else "project \(.project)" end): \(.describe)",
      "  \(.summary) (nothing applied)",
      (.replay.would_break[:10][] | "  would break: \(.src) → \(.dst) \(.proto | ascii_downcase)/\(.port) (\(.flows) flows)"),
      (.replay.would_allow[:10][] | "  would allow: \(.src) → \(.dst) \(.proto | ascii_downcase)/\(.port) (\(.flows) flows)")' <<<"$1"
}

np_project_pending_print() {
    if [[ "${NP_JSON:-0}" == 1 ]]; then jq . <<<"$1"; return; fi
    jq -r '"requested: \(.pending.label)\(if .preview then " — " + .preview else "" end)",
      "  waiting for another admin: netpol project approve \(.pending.id)  (or Approvals in the UI)"' <<<"$1"
}

np_project_put() {
    local p=$1 body=$2 out path
    path="/vm-network-policies/projects/$(np_uri "$p")"
    case "${NP_PMODE:-apply}" in
        preview)
            np_project_preview_print "$(np_api POST "$path/preview" -H 'Content-Type: application/json' -d "$body")"
            return ;;
        propose) path="$path?propose=1" ;;
    esac
    out=$(np_api PUT "$path" -H 'Content-Type: application/json' -d "$body")
    if [[ "$(jq -r '.pending.id // empty' <<<"$out")" != "" ]]; then np_project_pending_print "$out"; return; fi
    if [[ "${NP_JSON:-0}" == 1 ]]; then jq . <<<"$out"; return; fi
    jq -r '"\(if .project.project == "*" then "default" else "project \(.project.project)" end): \(.project.isolation)\(if .project.isolation == "isolated" and (.project.allow_host | not) then " (host blocked)" else "" end)\(if .project.egress_restricted then ", egress limited to \(.project.egress_allow | length) destination(s)" else "" end)\(if (.project.egress_ips | length) > 0 then ", egress IPs " + ([.project.egress_ips | to_entries[] | "\(.value)@\(.key)"] | join(", ")) else "" end)",
      (.sync[]? | select(.ok | not) | "  warning: \(.hostname): \(.error)")' <<<"$out"
}

np_projects_list() {
    np_need_fleet
    local body
    body=$(np_api GET /vm-network-policies/projects)
    if [[ "${NP_JSON:-0}" == 1 ]]; then jq . <<<"$body"; return; fi
    jq -r '"default: \(.default.isolation)\(if .default.isolation == "isolated" and (.default.allow_host | not) then " (host blocked)" else "" end)"' <<<"$body"
    if [[ "$(jq '.items | length' <<<"$body")" == 0 ]]; then echo "(no projects)" >&2; return; fi
    {
        printf 'PROJECT\tVMS\tISOLATION\tEGRESS\tEGRESS IPS\tPOLICIES\n'
        jq -r '.items[] | [
            .project,
            (.vms | length | tostring),
            ((if .isolated then "isolated" else "open" end) + (if .isolated and (.allow_host | not) then " (no host)" else "" end) + (if .settings.isolation == "inherit" then " *" else "" end)),
            (if .settings.egress_restricted then "\(.settings.egress_allow | length) allowed" else "any" end),
            ([.settings.egress_ips | to_entries[] | "\(.value)@\(.key)"] | join(",") | if . == "" then "-" else . end),
            (.policies | join(",") | if . == "" then "-" else . end)
          ] | @tsv' <<<"$body"
    } | column -t -s $'\t'
    echo "(* = follows the default)" >&2
    jq -r '.warnings[]? | "warning: \(.)"' <<<"$body" >&2
}

np_netpol_project() {
    np_project_mode "$@"
    set -- ${NP_ARGS[@]+"${NP_ARGS[@]}"}
    local sub="${1:-}" p host=true
    shift || true
    case "$sub" in
        ""|help|-h|--help) np_project_usage; return ;;
    esac
    np_need_fleet
    case "$sub" in
        isolate|open|inherit)
            p="${1:?usage: netpol project $sub PROJECT}"; shift
            [[ "${1:-}" == --no-host ]] && host=false
            local iso=$sub
            [[ "$sub" == isolate ]] && iso=isolated
            np_project_put "$p" "$(np_project_settings "$p" | jq -c --arg i "$iso" --argjson h "$host" '.isolation = $i | .allow_host = $h')"
            ;;
        reset)
            p="${1:?usage: netpol project reset PROJECT}"
            local path out
            path="/vm-network-policies/projects/$(np_uri "$p")"
            if [[ "$NP_PMODE" == preview ]]; then
                np_project_preview_print "$(np_api POST "$path/preview" -H 'Content-Type: application/json' -d '{}')"
                return
            fi
            [[ "$NP_PMODE" == propose ]] && path="$path?propose=1"
            out=$(np_api DELETE "$path")
            if [[ "$(jq -r '.pending.id // empty' <<<"$out")" != "" ]]; then np_project_pending_print "$out"; return; fi
            jq -r 'if .removed then "project \(.project): settings removed, follows the default" else "project \(.project) had no settings" end' <<<"$out"
            ;;
        default)
            local iso="${1:?usage: netpol project default isolated|open [--no-host]}"; shift
            [[ "$iso" == isolated || "$iso" == open ]] || np_die "default is isolated or open"
            [[ "${1:-}" == --no-host ]] && host=false
            np_project_put '*' "$(jq -nc --arg i "$iso" --argjson h "$host" '{isolation: $i, allow_host: $h}')"
            ;;
        assign)
            p="${1:?usage: netpol project assign PROJECT|- VM...}"; shift
            [[ $# -gt 0 ]] || np_die "usage: netpol project assign PROJECT|- VM..."
            [[ "$p" == - ]] && p=""
            local vm
            for vm in "$@"; do
                np_api PATCH "/vms/$(np_vm_ref "$vm")" -H 'Content-Type: application/json' -d "$(jq -nc --arg p "$p" '{project: $p}')" >/dev/null
                echo "vm/$vm project ${p:-cleared}"
            done
            ;;
        approve|reject)
            local id="${1:?usage: netpol project $sub ID}" verb=execute done=approved
            [[ "$sub" == reject ]] && verb=reject done=rejected
            np_api POST "/ai/actions/$(np_uri "$id")/$verb" -H 'Content-Type: application/json' -d '{}' >/dev/null
            echo "project change $id $done"
            ;;
        *) np_die "unknown project command: $sub (try: netpol project help)" ;;
    esac
}

np_netpol_overlay() {
    [[ "$NP_FLEET" == 1 ]] || np_die "the overlay is a fleet feature: add --fleet"
    local sub="${1:-status}" body='{}'
    shift || true
    case "$sub" in
        help|-h|--help)
            cat <<'EOF'
Usage: netpol overlay [status]                      (fleet)
       netpol overlay enable [--prefix4 CIDR] [--prefix6 CIDR] [--port N]
       netpol overlay disable

Encrypted WireGuard tunnels between hosts. Each host gets a fleet prefix
(default a /24 of 100.96.0.0/12 and a /64 of fd6d:6163:6869::/48) and
each VM address a fleet address in it. VMs reach VMs on other hosts by
fleet address; the receiving host sees the sending VM's fleet address,
so policies and project isolation apply across hosts even on NAT
networks. Needs wireguard-tools on every host; UDP 51871 between hosts.
EOF
            return ;;
        status) ;;
        enable|on|disable|off)
            body=$(jq -n --argjson e "$([[ "$sub" == enable || "$sub" == on ]] && echo true || echo false)" '{enabled: $e}')
            while [[ $# -gt 0 ]]; do
                case "$1" in
                    --prefix4) body=$(jq -c --arg v "${2:?}" '.prefix4 = $v' <<<"$body"); shift 2 ;;
                    --prefix6) body=$(jq -c --arg v "${2:?}" '.prefix6 = $v' <<<"$body"); shift 2 ;;
                    --port) body=$(jq -c --argjson v "${2:?}" '.port = $v' <<<"$body"); shift 2 ;;
                    *) np_die "unknown option: $1" ;;
                esac
            done
            np_api PUT /vm-network-policies/overlay -H 'Content-Type: application/json' -d "$body" >/dev/null
            ;;
        *) np_die "usage: netpol overlay [status|enable|disable] (netpol overlay help)" ;;
    esac
    local out
    out=$(np_api GET /vm-network-policies/overlay)
    if [[ "${NP_JSON:-0}" == 1 ]]; then jq . <<<"$out"; return; fi
    jq -r '.settings | "overlay: \(if .enabled then "on" else "off" end) (\(.prefix4), \(.prefix6), udp/\(.port))"' <<<"$out"
    [[ "$(jq '.items | length' <<<"$out")" == 0 ]] && return
    {
        printf 'HOST\tPREFIXES\tVMS\tPEERS\tHANDSHAKES\tERROR\n'
        jq -r '.items[] | [
            (.hostname // "-"),
            (.prefixes | join(",") | if . == "" then "-" else . end),
            (.mappings | length | tostring),
            (.peers | length | tostring),
            ([.peers[] | select(.latest_handshake > 0)] | length | tostring),
            (.error // "-")
          ] | @tsv' <<<"$out"
    } | column -t -s $'\t'
    jq -r '[.items[] | .hostname as $h | .mappings[] | "  \(.vm // "?") \(.local) → \(.fleet)  (\($h))"] | if length > 0 then "fleet addresses:", .[] else empty end' <<<"$out"
    jq -r '.errors[]? | "warning: \(.hostname): \(.error)"' <<<"$out" >&2
}

np_netpol_egress() {
    np_need_fleet
    np_project_mode "$@"
    set -- ${NP_ARGS[@]+"${NP_ARGS[@]}"}
    local p="${1:-}"
    case "$p" in
        help|-h|--help) np_project_usage; return ;;
        "")
            local body
            body=$(np_api GET /vm-network-policies/egress-ips)
            if [[ "${NP_JSON:-0}" == 1 ]]; then jq . <<<"$body"; return; fi
            {
                printf 'HOST\tACTIVE\tPROJECT\tEGRESS IP\tSOURCES\n'
                jq -r '.items[] | . as $h | if (.rules | length) == 0 then [.hostname, (if .active then "yes" else "no" end), "-", "-", "-"] | @tsv
                  else .rules[] | [$h.hostname, (if $h.active then "yes" else "no" end), .project, .egress_ip, (.sources | join(","))] | @tsv end' <<<"$body"
            } | column -t -s $'\t'
            jq -r '.items[] | (.skipped[]? as $s | "  \(.hostname): skipped \($s)"), (select(.error) | "  \(.hostname): error \(.error)")' <<<"$body" >&2
            jq -r '.errors[]? | "  \(.hostname): \(.error)"' <<<"$body" >&2
            return
            ;;
    esac
    shift
    local cur sub="${1:-show}"
    shift || true
    cur=$(np_project_settings "$p")
    case "$sub" in
        show)
            if [[ "${NP_JSON:-0}" == 1 ]]; then jq . <<<"$cur"; return; fi
            jq -r '"project \(.project): egress \(if .egress_restricted then "limited to the allowlist" else "unrestricted" end)",
              (.egress_allow[] | "  allow \(.to)\(if (.ports // []) | length > 0 then " on " + (.ports | join(", ")) else "" end)"),
              (.egress_ips | to_entries[] | "  egress IP \(.value) on \(.key)"),
              (if .egress_ip_required then "  hosts without an egress IP: internet egress blocked" else empty end)' <<<"$cur"
            ;;
        allow)
            local to="${1:?usage: netpol egress P allow TO [--port N]...}" ports='[]'
            shift
            while [[ $# -gt 0 ]]; do
                case "$1" in
                    --port|-p) ports=$(jq -c --arg v "$2" '. + [$v]' <<<"$ports"); shift 2 ;;
                    *) np_die "unknown option: $1" ;;
                esac
            done
            np_project_put "$p" "$(jq -c --arg t "$to" --argjson ps "$ports" '.egress_restricted = true | .egress_allow = ([.egress_allow[] | select(.to != $t)] + [{to: $t, ports: $ps}])' <<<"$cur")"
            ;;
        remove|rm)
            local to="${1:?usage: netpol egress P remove TO}"
            np_project_put "$p" "$(jq -c --arg t "$to" '.egress_allow = [.egress_allow[] | select(.to != $t)]' <<<"$cur")"
            ;;
        restrict) np_project_put "$p" "$(jq -c '.egress_restricted = true' <<<"$cur")" ;;
        unrestrict) np_project_put "$p" "$(jq -c '.egress_restricted = false' <<<"$cur")" ;;
        ip)
            local h="${1:?usage: netpol egress P ip HOST IP|-}" ip="${2:?usage: netpol egress P ip HOST IP|-}"
            if [[ "$ip" == - ]]; then
                np_project_put "$p" "$(jq -c --arg h "$h" 'del(.egress_ips[$h])' <<<"$cur")"
            else
                np_project_put "$p" "$(jq -c --arg h "$h" --arg ip "$ip" '.egress_ips[$h] = $ip' <<<"$cur")"
            fi
            ;;
        require-ip) np_project_put "$p" "$(jq -c '.egress_ip_required = true' <<<"$cur")" ;;
        allow-host-ip) np_project_put "$p" "$(jq -c '.egress_ip_required = false' <<<"$cur")" ;;
        *) np_die "unknown egress command: $sub (try: netpol egress help)" ;;
    esac
}

# ── DNS threat feeds ──────────────────────────────────────────────────────

np_threat_usage() {
    cat <<'EOF'
Usage: netpol threat [list]
       netpol threat set NAME (--url URL | -f FILE | --domain D [--domain D ...]) [--block]
       netpol threat refresh NAME
       netpol threat rm NAME

Domain lists checked against every VM's DNS replies (a domain covers its
subdomains). A match raises a threat_domain alert; with --block the answer
addresses are also denied as egress for every VM (AUDIT in observe mode,
dropped under the enforcement lease) for the record's TTL (10 min to 1 day).
Lists: one domain per line, hosts files, Adblock (||d^) or URLs. URL feeds
are fetched again every 12h. With --fleet, feeds go to every host.
EOF
}

np_threat_list() {
    local body
    body=$(np_api GET /vm-network-policies/threat-feeds)
    if [[ "${NP_JSON:-0}" == 1 ]]; then jq . <<<"$body"; return; fi
    if [[ "$(jq '.feeds | length' <<<"$body")" == 0 ]]; then
        echo "(no threat feeds)" >&2
        return
    fi
    {
        printf 'FEED\tDOMAINS\tMODE\tSOURCE\tUPDATED\n'
        jq -r '.feeds[] | [
            .name, ((.domains // .domain_count) | tostring), (if .block then "block" else "alert" end),
            (.source // "" | if . == "" then "inline" else . end), (.updated // .updated_at // "-")
          ] | @tsv' <<<"$body"
    } | column -t -s $'\t'
    echo "watching DNS of $(jq -r '.watched_vms // 0' <<<"$body") VM(s)"
    if [[ "$(jq '(.blocked // []) | length' <<<"$body")" != 0 ]]; then
        echo
        {
            printf 'BLOCKED\tDOMAIN\tFEED\tVM\tHOST\tLEFT\n'
            jq -r '.blocked[] | [.address, .domain, .feed, (.vm // "-"), (.hostname // "-"), "\(.expires_in_secs)s"] | @tsv' <<<"$body"
        } | column -t -s $'\t'
    fi
    jq -r '(.errors // [])[] | "\(.hostname): \(.error)"' <<<"$body" >&2
}

np_threat_set() {
    local name="${1:-}"
    [[ -n "$name" && "$name" != -* ]] || { np_threat_usage >&2; exit 1; }
    shift
    local url="" file="" block=false domains=()
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --url) url="$2"; shift 2 ;;
            -f|--file) file="$2"; shift 2 ;;
            --domain) domains+=("$2"); shift 2 ;;
            --block) block=true; shift ;;
            -h|--help) np_threat_usage; return ;;
            *) np_die "unknown option: $1" ;;
        esac
    done
    local req
    if [[ -n "$url" ]]; then
        req=$(jq -nc --arg u "$url" --argjson b "$block" '{url: $u, block: $b}')
    elif [[ -n "$file" ]]; then
        [[ "$file" == - || -r "$file" ]] || np_die "cannot read $file"
        req=$(np_read_file "$file" | jq -Rsc --argjson b "$block" '{text: ., block: $b}')
    elif [[ ${#domains[@]} -gt 0 ]]; then
        req=$(printf '%s\n' "${domains[@]}" | jq -Rnc --argjson b "$block" '{domains: [inputs], block: $b}')
    else
        np_threat_usage >&2
        exit 1
    fi
    local body
    body=$(np_api PUT "/vm-network-policies/threat-feeds/$(np_uri "$name")" -H 'Content-Type: application/json' --data-binary @- <<<"$req")
    if [[ "${NP_JSON:-0}" == 1 ]]; then jq . <<<"$body"; return; fi
    jq -r --arg n "$name" '
      (.feed // .) as $f
      | "threatfeed/\($n) set: \($f.domains // $f.domain_count) domains, \(if $f.block then "blocking" else "alert only" end)",
        ((.hosts // [])[] | "  on \(.hostname)"),
        ((.errors // [])[] | "  \(.hostname): \(.error)")' <<<"$body"
}

np_netpol_threat() {
    local sub="${1:-list}"
    shift || true
    case "$sub" in
        list|ls) np_threat_list ;;
        set|add|apply) np_threat_set "$@" ;;
        refresh)
            np_api POST "/vm-network-policies/threat-feeds/$(np_uri "${1:?usage: netpol threat refresh NAME}")/refresh" \
                -H 'Content-Type: application/json' -d '{}' \
                | jq -r --arg n "$1" '(.feed // .) as $f | "threatfeed/\($n) refreshed: \($f.domains // $f.domain_count) domains"'
            ;;
        rm|delete|remove)
            local body
            body=$(np_api DELETE "/vm-network-policies/threat-feeds/$(np_uri "${1:?usage: netpol threat rm NAME}")")
            if [[ "$(jq -r '.removed' <<<"$body")" == true ]]; then echo "threatfeed/$1 removed"; else echo "threatfeed/$1 not found"; fi
            ;;
        help|-h|--help) np_threat_usage ;;
        *) np_die "unknown threat command: $sub (try: netpol threat help)" ;;
    esac
}

# ── flows ─────────────────────────────────────────────────────────────────

np_flow_usage() {
    cat <<'EOF'
Usage: flow <command> [filters]

  observe [-f|--follow] [--last N]   Packet flows, Hubble style (colors on a TTY;
                                     NO_COLOR or --color never to disable)
  top [--by pair|src|dst|port|policy|vm|drop|l7] [--limit N]
                                     Busiest flows over the recent window
  stats                              Verdict / protocol / direction / drop-reason
                                     breakdown with bar charts
  edges [--vm NAME] [-o json]        7-day history: who talks to whom, per port and
                                     verdict, with L7 rate / status / latency
  alerts [--limit N]                 Port scans, host sweeps, deny bursts, new peers
  reset                              Clear the flow history (admin)

Filters:
  --vm NAME  --from-vm NAME  --to-vm NAME  --label k=v  --ip ADDR  --cidr PREFIX
  --port N  --protocol tcp|udp|icmp|sctp  --verdict FORWARDED,DROPPED,AUDIT
  --drop-reason policy-deny|default-deny|l7-deny|auth-required|spoofed-source
  --policy SUBSTR  --direction ingress|egress
  --host NAME (fleet)  -o json|compact  --color always|never|auto
EOF
}

NP_FLOW_JQ_TSV='[
  .ts, (.host // ""), (.src_vm // ""), .src, (.src_port|tostring), (.src_identity|tostring),
  (.dst_vm // ""), .dst, (.dst_port|tostring), (.dst_identity|tostring), .proto,
  (if .icmp_type != null then "type=\(.icmp_type)" else (.tcp_flags // "") end),
  .verdict, .direction, (.drop_reason // ""), (.policy // ""), (.bytes|tostring), (.iface // ""),
  (if .l7 then "\(.l7_type // "l7"): \(.l7)" else "" end)
] | map(if . == "" then "-" else gsub("\t"; " ") end) | @tsv'

# TSV (see NP_FLOW_JQ_TSV) → one colored line per flow.
np_flow_render() {
    local c=0
    np_color_on && c=1
    awk -F'\t' -v c="$c" -v fleet="$NP_FLEET" '
    function col(code, s) { return c ? "\033[" code "m" s "\033[0m" : s }
    function ep(name, addr, port, id,   s) {
        s = (name != "-") ? col("1;36", name) col("90", "(" addr ")") : col("36", addr)
        if (port != "0" && port != "-") s = s col("90", ":") col("36", port)
        if (id != "0" && id != "-" && id != "2") s = s col("90", " [" id "]")
        else if (id == "2") s = s col("90", " [world]")
        return s
    }
    {
        ts = $1; t = ts
        if (match(ts, /T[0-9:]+(\.[0-9]+)?/)) { t = substr(ts, RSTART + 1, RLENGTH - 1); if (length(t) > 12) t = substr(t, 1, 12) }
        line = col("90", t) "  "
        if (fleet == 1 && $2 != "-") line = line col("35", "[" $2 "]") " "
        line = line ep($3, $4, $5, $6) col("1;37", " → ") ep($7, $8, $9, $10)
        proto = toupper($11); flags = ($12 == "-") ? "" : " " $12
        line = line "  " col("33", proto) col("90", flags)
        v = $13
        if (v == "FORWARDED") vv = col("1;32", "✔ FORWARDED")
        else if (v == "DROPPED") vv = col("1;31", "✘ DROPPED")
        else if (v == "AUDIT") vv = col("1;33", "◉ AUDIT")
        else vv = v
        line = line "  " vv
        if ($15 != "-") line = line col("31", " (" $15 ")")
        line = line "  " col("34", $14)
        if ($16 != "-") line = line "  " col("2;37", "↳ " $16)
        if ($19 != "-" && $19 != "") line = line "  " col("1;35", "◆ " $19)
        print line
        fflush()
    }'
}

np_flow_query() {
    local q=""
    local k v
    while [[ $# -gt 1 ]]; do
        k=$1 v=$2
        shift 2
        [[ -n "$v" ]] && q+="&${k}=$(np_uri "$v")"
    done
    printf '%s' "${q#&}"
}

np_flow_banner() {
    local what=$1 filters=$2
    np_color_on || { echo "# $what${filters:+ ($filters)}"; return; }
    printf '\033[1;37m●\033[0m \033[1m%s\033[0m' "$what"
    [[ -n "$filters" ]] && printf '  \033[90m%s\033[0m' "$filters"
    printf '\n\033[90m%s\033[0m\n' "TIME          SOURCE → DESTINATION                         PROTO  VERDICT  DIR  POLICY"
}

np_flow_edges() {
    local vm="" out="" body
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --vm) vm="$2"; shift 2 ;;
            -o|--output) out="$2"; shift 2 ;;
            --color) NP_COLOR="$2"; shift 2 ;;
            *) np_die "unknown option: $1" ;;
        esac
    done
    body=$(np_api GET "/flows/edges${vm:+?vm=$(np_uri "$vm")}")
    if [[ "$out" == json ]]; then jq . <<<"$body"; return; fi
    {
        printf 'SOURCE\tDESTINATION\tPORT\tDIR\tVERDICT\tFLOWS\tPOLICY\tLAST SEEN\tL7\n'
        jq -r '.items | sort_by(-.count)[] | [
            (.src_vm // .src), (.dst_vm // .dst), "\(.port)/\(.proto | ascii_downcase)", .direction,
            (.verdict + (if .drop_reason then "(\(.drop_reason))" else "" end)), (.count | tostring),
            (.policy // "-"), .last_seen,
            ((.l7 // [])[:3] | map("\(.request) ×\(.count)\(if .latency_n > 0 then " \(.latency_ms_total / .latency_n | floor)ms" else "" end)\(if (.status["5xx"] // 0) > 0 then " 5xx=\(.status["5xx"])" else "" end)") | join("; ") | if . == "" then "-" else . end)
          ] | @tsv' <<<"$body"
    } | column -t -s $'\t' | awk -v c="$(np_color_on && echo 1)" '
        NR == 1 { print c ? "\033[90m" $0 "\033[0m" : $0; next }
        /DROPPED/ { print c ? "\033[31m" $0 "\033[0m" : $0; next }
        /AUDIT/ { print c ? "\033[33m" $0 "\033[0m" : $0; next }
        { print }'
}

np_flow_alerts() {
    local limit=100 body
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --limit|-n) limit="$2"; shift 2 ;;
            *) np_die "unknown option: $1" ;;
        esac
    done
    body=$(np_api GET "/flows/alerts?limit=${limit}")
    if [[ "${NP_JSON:-0}" == 1 ]]; then jq . <<<"$body"; return; fi
    jq -r '.items[] | "\(.ts)  \(if .severity == "high" then "\u001b[1;31m" elif .severity == "medium" then "\u001b[33m" else "\u001b[36m" end)\(.severity | ascii_upcase)\u001b[0m  \u001b[1m\(.kind)\u001b[0m  \(.src_vm // .src)\(if .host then "  \u001b[35m[\(.host)]\u001b[0m" else "" end)  \(.detail)"' <<<"$body" | np_strip
    [[ "$(jq '.items | length' <<<"$body")" != 0 ]] || echo "(no alerts)" >&2
}

np_flow_main() {
    np_need
    local sub="${1:-observe}"
    case "$sub" in
        edges|history) shift; np_flow_edges "$@"; return ;;
        alerts) shift; np_flow_alerts "$@"; return ;;
        reset) np_api DELETE /flows/edges >/dev/null; echo "flow history cleared"; return ;;
    esac
    case "$sub" in -*) sub=observe ;; *) shift || true ;; esac
    local follow=0 last=50 out="" by="pair" limit=15
    local vm="" from_vm="" to_vm="" label="" ip="" cidr="" port="" protocol="" verdict="" drop="" policy="" direction="" host=""
    while [[ $# -gt 0 ]]; do
        case "$1" in
            -f|--follow) follow=1; shift ;;
            --last|-n) last="$2"; shift 2 ;;
            -o|--output) out="$2"; shift 2 ;;
            --by) by="$2"; shift 2 ;;
            --limit) limit="$2"; shift 2 ;;
            --vm) vm="$2"; shift 2 ;;
            --from-vm|--from) from_vm="$2"; shift 2 ;;
            --to-vm|--to) to_vm="$2"; shift 2 ;;
            --label) label="$2"; shift 2 ;;
            --ip) ip="$2"; shift 2 ;;
            --cidr) cidr="$2"; shift 2 ;;
            --port) port="$2"; shift 2 ;;
            --protocol|--proto) protocol="$2"; shift 2 ;;
            --verdict) verdict="$2"; shift 2 ;;
            --drop-reason) drop="$2"; shift 2 ;;
            --policy) policy="$2"; shift 2 ;;
            --direction) direction="$2"; shift 2 ;;
            --host) host="$2"; shift 2 ;;
            --color) NP_COLOR="$2"; shift 2 ;;
            --color=*) NP_COLOR="${1#--color=}"; shift ;;
            -h|--help) np_flow_usage; return ;;
            *) np_die "unknown option: $1 (try: flow help)" ;;
        esac
    done
    local fq
    fq=$(np_flow_query vm "$vm" from_vm "$from_vm" to_vm "$to_vm" label "$label" ip "$ip" cidr "$cidr" \
        port "$port" protocol "$protocol" verdict "$verdict" drop_reason "$drop" policy "$policy" \
        direction "$direction" host "$host")
    local filters
    filters=$(sed 's/&/ /g; s/%2C/,/g; s/%3D/=/g; s/%2F/\//g' <<<"$fq")

    case "$sub" in
        observe)
            if [[ "$follow" == 1 ]]; then
                [[ -z "$out" ]] && np_flow_banner "flow observe --follow" "$filters"
                local url="${NP_BASE}/flows/stream?last=${last}${fq:+&$fq}"
                local parse='select(startswith("data:")) | sub("^data: ?"; "") | select(length > 0) | fromjson'
                case "$out" in
                    json) curl -skN "${NP_AUTH[@]}" "$url" | jq -R --unbuffered -c "$parse" ;;
                    *) curl -skN "${NP_AUTH[@]}" "$url" | jq -R --unbuffered -r "$parse | $NP_FLOW_JQ_TSV" | np_flow_render ;;
                esac
            else
                local body
                body=$(np_api GET "/flows?limit=${last}${fq:+&$fq}")
                case "$out" in
                    json) jq -c '.items | reverse | .[]' <<<"$body" ;;
                    *)
                        np_flow_banner "flow observe" "$filters"
                        jq -r ".items | reverse | .[] | $NP_FLOW_JQ_TSV" <<<"$body" | np_flow_render
                        if [[ "$(jq '.items | length' <<<"$body")" == 0 ]]; then
                            echo "(no flows — is a VM network policy applied and the bpfd VM edge attached?)" >&2
                        fi
                        ;;
                esac
            fi
            ;;
        top) np_flow_top "$fq" "$by" "$limit" ;;
        stats) np_flow_stats "$fq" ;;
        help) np_flow_usage ;;
        *) np_die "unknown flow command: $sub (try: flow help)" ;;
    esac
}

# Horizontal bar: green forwarded, yellow audit, red dropped.
NP_BAR_AWK='
function col(code, s) { return c ? "\033[" code "m" s "\033[0m" : s }
function rep(ch, n,   s) { s = ""; while (n-- > 0) s = s ch; return s }
function bar(f, a, d, max, width,   wf, wa, wd) {
    if (max <= 0) return ""
    wf = int(f * width / max + 0.5); wa = int(a * width / max + 0.5); wd = int(d * width / max + 0.5)
    if (f > 0 && wf == 0) wf = 1; if (a > 0 && wa == 0) wa = 1; if (d > 0 && wd == 0) wd = 1
    return col("32", rep("█", wf)) col("33", rep("█", wa)) col("31", rep("█", wd))
}'

np_flow_top() {
    local fq=$1 by=$2 limit=$3 body c=0
    np_color_on && c=1
    body=$(np_api GET "/flows?limit=5000${fq:+&$fq}")
    local key
    case "$by" in
        pair) key='"\(.src_vm // .src) → \(.dst_vm // .dst):\(.dst_port)/\(.proto)"' ;;
        src) key='(.src_vm // .src)' ;;
        dst) key='"\(.dst_vm // .dst):\(.dst_port)/\(.proto)"' ;;
        port) key='"\(.dst_port)/\(.proto)"' ;;
        policy) key='(.policy // "(no policy)")' ;;
        vm) key='(.vm // "-")' ;;
        drop) key='(.drop_reason // "-")' ;;
        l7) key='(if .l7 then "\(.l7_type // "l7"): \(.l7)" else "(no L7)" end)' ;;
        *) np_die "--by must be pair|src|dst|port|policy|vm|drop|l7" ;;
    esac
    np_color_on && printf '\033[1m● flow top --by %s\033[0m  \033[90m%s\033[0m\n' "$by" "$(jq '.items|length' <<<"$body") flows" \
        || echo "# flow top --by $by"
    jq -r --argjson lim "$limit" ".items | group_by($key) | map({k: (.[0] | $key), n: length,
            f: (map(select(.verdict == \"FORWARDED\")) | length),
            a: (map(select(.verdict == \"AUDIT\")) | length),
            d: (map(select(.verdict == \"DROPPED\")) | length),
            b: (map(.bytes) | add)}) | sort_by(-.n) | .[:\$lim][] | [.k, .n, .f, .a, .d, .b] | @tsv" <<<"$body" |
    awk -F'\t' -v c="$c" "$NP_BAR_AWK"'
    { k[NR] = $1; n[NR] = $2; f[NR] = $3; a[NR] = $4; d[NR] = $5; b[NR] = $6; if ($2 > max) max = $2; if (length($1) > kw) kw = length($1) }
    END {
        if (NR == 0) { print "(no flows)"; exit }
        if (kw > 60) kw = 60
        printf "%s\n", col("90", sprintf("%-" kw "s  %6s  %6s  %6s  %6s  %s", "KEY", "FLOWS", "FWD", "AUDIT", "DROP", ""))
        for (i = 1; i <= NR; i++)
            printf "%-" kw "s  %6d  %s  %s  %s  %s\n", substr(k[i], 1, kw), n[i],
                col("32", sprintf("%6d", f[i])), col("33", sprintf("%6d", a[i])), col("31", sprintf("%6d", d[i])), bar(f[i], a[i], d[i], max, 30)
    }'
}

np_flow_stats() {
    local fq=$1 body c=0
    np_color_on && c=1
    body=$(np_api GET "/flows?limit=5000${fq:+&$fq}")
    jq -r '.items as $i |
      ("verdict", "proto", "direction", "drop_reason", "policy") as $f |
      ($i | group_by(.[$f] // "-") | map([$f, (.[0][$f] // "-"), length]) | sort_by(-.[2]) | .[:8][]) | @tsv' <<<"$body" |
    awk -F'\t' -v c="$c" -v total="$(jq '.items|length' <<<"$body")" "$NP_BAR_AWK"'
    { s[NR] = $1; k[NR] = $2; n[NR] = $3; if ($3 > max[$1]) max[$1] = $3 }
    END {
        printf "%s %s\n", col("1", "● flow stats"), col("90", total " flows (most recent window)")
        if (NR == 0) { print "(no flows)"; exit }
        for (i = 1; i <= NR; i++) {
            if (s[i] != prev) { h = s[i]; gsub(/_/, " ", h); printf "\n%s\n", col("1;37", toupper(substr(h, 1, 1)) substr(h, 2)); prev = s[i] }
            code = "36"
            if (k[i] == "FORWARDED") code = "32"; else if (k[i] == "DROPPED" || s[i] == "drop_reason" && k[i] != "-") code = "31"; else if (k[i] == "AUDIT") code = "33"
            pct = total > 0 ? n[i] * 100 / total : 0
            w = max[s[i]] > 0 ? int(n[i] * 30 / max[s[i]] + 0.5) : 0
            printf "  %-34s %6d %5.1f%%  %s\n", substr(k[i], 1, 34), n[i], pct, col(code, rep("█", w))
        }
    }'
}
