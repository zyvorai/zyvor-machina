#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

# Bare-metal provisioning commands (`machinactl metal ...`). Uses the helpers from
# netpol-ctl.sh (np_api, np_die, np_need) and always talks to the controller.

metal_usage() {
    cat <<'USAGE'
machinactl metal <command>
  list                                     Servers: lifecycle, zone, power state
  add HOSTNAME --bmc ADDR [--type redfish|ipmi|fake] [--user U] [--mac MAC] [--zone Z]
                                           (BMC password from $MACHINA_BMC_PASSWORD)
  bmc-set ID --type redfish|ipmi|libvirt|fake --address A [--user U] [--boot-method pxe|virtual_media|virtual_media_uefi]
                                           Attach BMC details (auto-enrolled machines have none);
                                           password from $MACHINA_BMC_PASSWORD, empty keeps the stored one
  commission ID                            Start commissioning; prints the one-time token
  inventory ID                             Hardware report from commissioning
  power ID on|off|cycle|reset [--dry-run]  Power through the BMC
  allocate [--zone Z] [--cores N] [--memory-mib N]
  deploy ID --os ubuntu-24.04 --key "ssh-ed25519 AAAA…" [--timezone UTC]
  release ID                               allocated|deployed -> ready
  lifecycle ID STATE                       Move to retired, new, ready, ...
  image list|catalog                       Imported OS images / names that can be imported
  image add NAME [--url U --checksum HEX|--sums-url U] [--algo sha256|sha512]
                                           NAME alone imports a catalog image (ubuntu-26.04, ...)
  image rm NAME
  profile list|add|rm                      Deploy profiles
  profile add NAME --image IMG --key "ssh-ed25519 …" [--zone Z] [--cores N] [--memory-mib N]
                    [--timezone TZ] [--auto --count N]
  provision --profile NAME [--count N]     Allocate and deploy N machines in one step
  discover --cidr 10.20.0.0/24 [--import] [--prefix bmc-] [--zone Z] [--scheme https|http] [--port N]
                                           Scan for Redfish BMCs; credentials from $MACHINA_BMC_CREDENTIALS
                                           ("user:pass,user2:pass2", at most 3 are tried per BMC)
  import --csv FILE                        hostname,bmc_address,bmc_type,username,password,mac,zone
  zones                                    Servers per zone
  dnsmasq --subnet CIDR-NET [--interface IF] [--tftp-root DIR]
                                           dnsmasq.conf for a site's boot host
USAGE
}

metal_main() {
    np_need
    local sub="${1:-}"
    shift || true
    case "$sub" in
        list | ls)
            np_api GET /baremetal/servers | jq -r '
              (["ID","HOSTNAME","LIFECYCLE","ZONE","POWER","MAC","BMC","CORES","MEM_MIB","ERROR"] | @tsv),
              (.[] | [.id, .hostname, .lifecycle, .zone, .state, .mac_address, .bmc_address,
                      .cpu_cores, .memory_mib, .last_error] | @tsv)' | column -t -s $'\t'
            ;;
        add)
            local host="${1:-}" bmc="" type=redfish user="" mac="" zone=""
            [[ -n "$host" ]] || np_die "usage: metal add HOSTNAME --bmc ADDR"
            shift
            while [[ $# -gt 0 ]]; do
                case "$1" in
                    --bmc) bmc=${2:-}; shift 2 ;;
                    --type) type=${2:-}; shift 2 ;;
                    --user) user=${2:-}; shift 2 ;;
                    --mac) mac=${2:-}; shift 2 ;;
                    --zone) zone=${2:-}; shift 2 ;;
                    *) np_die "unknown option $1" ;;
                esac
            done
            # Password only through the environment, and into the request body via jq.
            BMC_PW="${MACHINA_BMC_PASSWORD:-}" jq -n --arg h "$host" --arg b "$bmc" --arg t "$type" \
                --arg u "$user" --arg m "$mac" --arg z "$zone" \
                '{hostname:$h, bmc_address:$b, bmc_type:$t, bmc_username:$u, bmc_password:(env.BMC_PW // ""),
                  mac_address:$m, zone:$z}' |
                np_api POST /baremetal/servers -H 'Content-Type: application/json' --data-binary @- |
                jq '{id, hostname, lifecycle, zone, mac_address, bmc_credentials_configured}'
            ;;
        bmc-set)
            local id="${1:-}" type="" addr="" user="" bm=""
            [[ -n "$id" ]] || np_die "usage: metal bmc-set ID --type T --address A [--user U]"
            shift
            while [[ $# -gt 0 ]]; do
                case "$1" in
                    --type) type=${2:-}; shift 2 ;;
                    --address) addr=${2:-}; shift 2 ;;
                    --user) user=${2:-}; shift 2 ;;
                    --boot-method) bm=${2:-}; shift 2 ;;
                    *) np_die "unknown option $1" ;;
                esac
            done
            [[ -n "$type" ]] || np_die "bmc-set needs --type"
            BMC_PW="${MACHINA_BMC_PASSWORD:-}" jq -n --arg t "$type" --arg a "$addr" --arg u "$user" --arg b "$bm" \
                '{bmc_type:$t, bmc_address:$a, bmc_username:$u, bmc_password:(env.BMC_PW // ""), boot_method:$b}' |
                np_api PUT "/baremetal/servers/$id/bmc" -H 'Content-Type: application/json' --data-binary @- | jq .
            ;;
        commission)
            [[ -n "${1:-}" ]] || np_die "usage: metal commission ID"
            np_api POST "/baremetal/servers/$1/commission" | jq .
            ;;
        inventory)
            [[ -n "${1:-}" ]] || np_die "usage: metal inventory ID"
            np_api GET "/baremetal/servers/$1/inventory" | jq .
            ;;
        power)
            local id="${1:-}" action="${2:-}" dry=false
            [[ -n "$id" && -n "$action" ]] || np_die "usage: metal power ID on|off|cycle|reset [--dry-run]"
            [[ "${3:-}" == "--dry-run" ]] && dry=true
            jq -n --arg a "$action" --argjson d "$dry" '{action:$a, dry_run:$d}' |
                np_api POST "/baremetal/servers/$id/power" -H 'Content-Type: application/json' --data-binary @- | jq .
            ;;
        allocate)
            local zone="" cores=0 mem=0
            while [[ $# -gt 0 ]]; do
                case "$1" in
                    --zone) zone=${2:-}; shift 2 ;;
                    --cores) cores=${2:-0}; shift 2 ;;
                    --memory-mib) mem=${2:-0}; shift 2 ;;
                    *) np_die "unknown option $1" ;;
                esac
            done
            jq -n --arg z "$zone" --argjson c "$cores" --argjson m "$mem" \
                '{zone:$z, min_cpu_cores:$c, min_memory_mib:$m}' |
                np_api POST /baremetal/allocate -H 'Content-Type: application/json' --data-binary @- | jq .
            ;;
        deploy)
            local id="${1:-}" os="" tz=UTC keys=()
            [[ -n "$id" ]] || np_die "usage: metal deploy ID --os OS --key KEY"
            shift
            while [[ $# -gt 0 ]]; do
                case "$1" in
                    --os) os=${2:-}; shift 2 ;;
                    --key) keys+=("${2:-}"); shift 2 ;;
                    --timezone) tz=${2:-}; shift 2 ;;
                    *) np_die "unknown option $1" ;;
                esac
            done
            [[ -n "$os" && ${#keys[@]} -gt 0 ]] || np_die "deploy needs --os and at least one --key"
            printf '%s\n' "${keys[@]}" | jq -R . | jq -s --arg o "$os" --arg t "$tz" \
                '{os:$o, ssh_authorized_keys:., timezone:$t}' |
                np_api POST "/baremetal/servers/$id/deploy" -H 'Content-Type: application/json' --data-binary @- | jq .
            ;;
        release)
            [[ -n "${1:-}" ]] || np_die "usage: metal release ID"
            np_api POST "/baremetal/servers/$1/release" | jq .
            ;;
        lifecycle)
            [[ -n "${1:-}" && -n "${2:-}" ]] || np_die "usage: metal lifecycle ID STATE"
            jq -n --arg to "$2" '{to:$to}' |
                np_api POST "/baremetal/servers/$1/lifecycle" -H 'Content-Type: application/json' --data-binary @- | jq .
            ;;
        image | images)
            local isub="${1:-list}"
            shift || true
            case "$isub" in
                list | ls)
                    np_api GET /baremetal/images | jq -r '
                      (["NAME","OS","ARCH","STATUS","SIZE_MIB","CHECKSUM","ERROR"] | @tsv),
                      (.images[] | [.name, .os, .arch, .status, ((.size_bytes / 1048576) | floor),
                                    (.checksum[0:16] + (if .checksum == "" then "" else "…" end)), .error] | @tsv)' |
                        column -t -s $'\t'
                    ;;
                catalog)
                    np_api GET /baremetal/images | jq -r '.catalog[] | [.name, .arch, .url] | @tsv' | column -t -s $'\t'
                    ;;
                add)
                    local name="${1:-}" url="" sum="" sums="" algo=""
                    [[ -n "$name" ]] || np_die "usage: metal image add NAME [--url U --checksum HEX]"
                    shift
                    while [[ $# -gt 0 ]]; do
                        case "$1" in
                            --url) url=${2:-}; shift 2 ;;
                            --checksum) sum=${2:-}; shift 2 ;;
                            --sums-url) sums=${2:-}; shift 2 ;;
                            --algo) algo=${2:-}; shift 2 ;;
                            *) np_die "unknown option $1" ;;
                        esac
                    done
                    jq -n --arg n "$name" --arg u "$url" --arg c "$sum" --arg s "$sums" --arg a "$algo" \
                        '{name:$n, url:$u, checksum:$c, sums_url:$s, checksum_algo:$a}' |
                        np_api POST /baremetal/images -H 'Content-Type: application/json' --data-binary @- |
                        jq '{name, status, source_url}'
                    echo "downloading in the background: machinactl metal image list" >&2
                    ;;
                rm | delete)
                    [[ -n "${1:-}" ]] || np_die "usage: metal image rm NAME"
                    np_api DELETE "/baremetal/images/$1" | jq .
                    ;;
                *) np_die "usage: metal image list|catalog|add|rm" ;;
            esac
            ;;
        profile | profiles)
            local psub="${1:-list}"
            shift || true
            case "$psub" in
                list | ls)
                    np_api GET /baremetal/profiles | jq -r '
                      (["NAME","IMAGE","ZONE","MIN_CORES","MIN_MEM_MIB","AUTO","COUNT"] | @tsv),
                      (.[] | [.name, .image, .zone, .min_cpu_cores, .min_memory_mib, .auto_deploy, .desired_count] | @tsv)' |
                        column -t -s $'\t'
                    ;;
                add)
                    local name="${1:-}" image="" zone="" cores=0 mem=0 tz=UTC auto=false count=0 keys=()
                    [[ -n "$name" ]] || np_die "usage: metal profile add NAME --image IMG --key KEY"
                    shift
                    while [[ $# -gt 0 ]]; do
                        case "$1" in
                            --image) image=${2:-}; shift 2 ;;
                            --key) keys+=("${2:-}"); shift 2 ;;
                            --zone) zone=${2:-}; shift 2 ;;
                            --cores) cores=${2:-0}; shift 2 ;;
                            --memory-mib) mem=${2:-0}; shift 2 ;;
                            --timezone) tz=${2:-}; shift 2 ;;
                            --auto) auto=true; shift ;;
                            --count) count=${2:-0}; shift 2 ;;
                            *) np_die "unknown option $1" ;;
                        esac
                    done
                    [[ -n "$image" && ${#keys[@]} -gt 0 ]] || np_die "profile add needs --image and at least one --key"
                    printf '%s\n' "${keys[@]}" | jq -R . | jq -s --arg n "$name" --arg i "$image" --arg z "$zone" \
                        --argjson c "$cores" --argjson m "$mem" --arg t "$tz" --argjson a "$auto" --argjson k "$count" \
                        '{name:$n, image:$i, ssh_keys:., zone:$z, min_cpu_cores:$c, min_memory_mib:$m,
                          timezone:$t, auto_deploy:$a, desired_count:$k}' |
                        np_api PUT /baremetal/profiles -H 'Content-Type: application/json' --data-binary @- | jq .
                    ;;
                rm | delete)
                    [[ -n "${1:-}" ]] || np_die "usage: metal profile rm NAME"
                    np_api DELETE "/baremetal/profiles/$1" | jq .
                    ;;
                *) np_die "usage: metal profile list|add|rm" ;;
            esac
            ;;
        provision)
            local prof="" count=1
            while [[ $# -gt 0 ]]; do
                case "$1" in
                    --profile) prof=${2:-}; shift 2 ;;
                    --count) count=${2:-1}; shift 2 ;;
                    *) np_die "unknown option $1" ;;
                esac
            done
            [[ -n "$prof" ]] || np_die "usage: metal provision --profile NAME [--count N]"
            jq -n --arg p "$prof" --argjson c "$count" '{profile:$p, count:$c}' |
                np_api POST /baremetal/provision -H 'Content-Type: application/json' --data-binary @- | jq .
            ;;
        discover)
            local cidr="" imp=false prefix="bmc-" zone="" scheme=https port=""
            while [[ $# -gt 0 ]]; do
                case "$1" in
                    --cidr) cidr=${2:-}; shift 2 ;;
                    --import) imp=true; shift ;;
                    --prefix) prefix=${2:-}; shift 2 ;;
                    --zone) zone=${2:-}; shift 2 ;;
                    --scheme) scheme=${2:-}; shift 2 ;;
                    --port) port=${2:-}; shift 2 ;;
                    *) np_die "unknown option $1" ;;
                esac
            done
            [[ -n "$cidr" ]] || np_die "usage: metal discover --cidr 10.20.0.0/24 [--import]"
            [[ -n "${MACHINA_BMC_CREDENTIALS:-}" ]] || np_die "set MACHINA_BMC_CREDENTIALS=\"user:pass,user2:pass2\""
            # Credentials go in the request body (stdin), never on a command line.
            python3 -c '
import json, os, sys
creds = []
for part in os.environ["MACHINA_BMC_CREDENTIALS"].split(","):
    u, _, p = part.partition(":")
    if u:
        creds.append({"username": u, "password": p})
a = sys.argv
body = {"cidr": a[1], "credentials": creds, "import": a[2] == "true", "hostname_prefix": a[3],
        "zone": a[4], "scheme": a[5]}
if a[6]:
    body["port"] = int(a[6])
print(json.dumps(body))' "$cidr" "$imp" "$prefix" "$zone" "$scheme" "$port" |
                np_api POST /baremetal/discover -H 'Content-Type: application/json' --data-binary @- |
                jq -r '"found \(.found|length) BMC(s), \(.authenticated) accepted a credential",
                       (.found[] | "  \(.address)\t\(.vendor) \(.product)\t\(.manufacturer) \(.model)\t\(.serial_number)\t" +
                                   (if .credential_index == null then "no credential accepted" else "credential #\(.credential_index + 1)" end)),
                       (.imported[]? | "  import \(.hostname): \(.status) \(.error)")'
            ;;
        import)
            local csv=""
            [[ "${1:-}" == "--csv" ]] && csv=${2:-}
            [[ -n "$csv" && -r "$csv" ]] || np_die "usage: metal import --csv FILE (hostname,bmc_address,bmc_type,username,password,mac,zone)"
            python3 -c '
import csv, json, sys
rows = []
with open(sys.argv[1], newline="") as f:
    for r in csv.DictReader(f):
        if not (r.get("hostname") or "").strip():
            continue
        rows.append({
            "hostname": r["hostname"].strip(), "bmc_address": (r.get("bmc_address") or "").strip(),
            "bmc_type": (r.get("bmc_type") or "redfish").strip(),
            "bmc_username": (r.get("username") or "").strip(), "bmc_password": r.get("password") or "",
            "mac_address": (r.get("mac") or "").strip(), "zone": (r.get("zone") or "").strip()})
print(json.dumps({"servers": rows}))' "$csv" |
                np_api POST /baremetal/import -H 'Content-Type: application/json' --data-binary @- |
                jq -r '"created \(.created), already there \(.exists), errors \(.errors)",
                       (.rows[] | select(.status == "error") | "  \(.hostname): \(.error)")'
            ;;
        zones)
            np_api GET /baremetal/zones | jq -r '
              (["ZONE","TOTAL","READY","ALLOCATED","DEPLOYED"] | @tsv),
              (.[] | [(if .zone == "" then "(none)" else .zone end), .total, .ready, .allocated, .deployed] | @tsv)' |
                column -t -s $'\t'
            ;;
        dnsmasq)
            local subnet="" iface="" root=/srv/tftp
            while [[ $# -gt 0 ]]; do
                case "$1" in
                    --subnet) subnet=${2:-}; shift 2 ;;
                    --interface) iface=${2:-}; shift 2 ;;
                    --tftp-root) root=${2:-}; shift 2 ;;
                    *) np_die "unknown option $1" ;;
                esac
            done
            [[ -n "$subnet" ]] || np_die "usage: metal dnsmasq --subnet NET"
            np_api GET "/baremetal/boot/dnsmasq.conf?subnet=$(np_uri "$subnet")&tftp_root=$(np_uri "$root")&interface=$(np_uri "$iface")"
            echo
            ;;
        "" | help | -h | --help) metal_usage ;;
        *) np_die "unknown metal command '$sub' (try: machinactl metal help)" ;;
    esac
}
