// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! tracefs `format` parsing: tracepoint field offsets and symbolic value tables.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use machina_bpf_common::tp;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Field {
    pub offset: u32,
    pub size: u32,
}

/// Parse `field:<type> <name>[N];\toffset:X;\tsize:Y;...` lines.
pub fn parse_format(text: &str) -> HashMap<String, Field> {
    let mut out = HashMap::new();
    for line in text.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("field:") else {
            continue;
        };
        let mut decl = "";
        let mut offset = None;
        let mut size = None;
        for (i, part) in rest.split(';').enumerate() {
            let part = part.trim();
            if i == 0 {
                decl = part;
            } else if let Some(v) = part.strip_prefix("offset:") {
                offset = v.trim().parse().ok();
            } else if let Some(v) = part.strip_prefix("size:") {
                size = v.trim().parse().ok();
            }
        }
        let Some(name) = decl.split_whitespace().last() else {
            continue;
        };
        let name = name
            .split('[')
            .next()
            .unwrap_or(name)
            .trim_start_matches('*');
        if let (Some(offset), Some(size)) = (offset, size) {
            out.insert(name.to_string(), Field { offset, size });
        }
    }
    out
}

/// Parse `{ 2, "NOT_SPECIFIED" }` pairs from a `print fmt:` line.
pub fn parse_symbolic(text: &str) -> HashMap<u32, String> {
    let mut out = HashMap::new();
    let Some(fmt) = text.lines().find(|l| l.starts_with("print fmt:")) else {
        return out;
    };
    let mut rest = fmt;
    while let Some(start) = rest.find('{') {
        rest = &rest[start + 1..];
        let Some(end) = rest.find('}') else { break };
        let inner = &rest[..end];
        rest = &rest[end + 1..];
        let Some((num, name)) = inner.split_once(',') else {
            continue;
        };
        let num = num.trim();
        let parsed = if let Some(h) = num.strip_prefix("0x") {
            u32::from_str_radix(h, 16).ok()
        } else {
            num.parse().ok()
        };
        let name = name.trim().trim_matches('"');
        if let Some(n) = parsed {
            if !name.is_empty() {
                out.insert(n, name.to_string());
            }
        }
    }
    out
}

pub fn tracefs_root() -> Option<PathBuf> {
    ["/sys/kernel/tracing", "/sys/kernel/debug/tracing"]
        .iter()
        .map(PathBuf::from)
        .find(|p| p.join("events").is_dir())
}

fn read_event(root: &Path, category: &str, name: &str) -> Option<String> {
    std::fs::read_to_string(root.join("events").join(category).join(name).join("format")).ok()
}

/// (TP_OFF index, category, event, field)
const OFFSETS: &[(u32, &str, &str, &str)] = &[
    (
        tp::EXEC_FILENAME_LOC,
        "sched",
        "sched_process_exec",
        "filename",
    ),
    (
        tp::FORK_PARENT_PID,
        "sched",
        "sched_process_fork",
        "parent_pid",
    ),
    (
        tp::FORK_CHILD_PID,
        "sched",
        "sched_process_fork",
        "child_pid",
    ),
    (
        tp::OPENAT_FILENAME,
        "syscalls",
        "sys_enter_openat",
        "filename",
    ),
    (tp::OPENAT_FLAGS, "syscalls", "sys_enter_openat", "flags"),
    (tp::ISS_OLDSTATE, "sock", "inet_sock_set_state", "oldstate"),
    (tp::ISS_NEWSTATE, "sock", "inet_sock_set_state", "newstate"),
    (tp::ISS_SPORT, "sock", "inet_sock_set_state", "sport"),
    (tp::ISS_DPORT, "sock", "inet_sock_set_state", "dport"),
    (tp::ISS_FAMILY, "sock", "inet_sock_set_state", "family"),
    (tp::ISS_PROTOCOL, "sock", "inet_sock_set_state", "protocol"),
    (tp::ISS_SADDR_V6, "sock", "inet_sock_set_state", "saddr_v6"),
    (tp::ISS_DADDR_V6, "sock", "inet_sock_set_state", "daddr_v6"),
    (tp::KFREE_REASON, "skb", "kfree_skb", "reason"),
    (
        tp::RETRANS_SADDR_V6,
        "tcp",
        "tcp_retransmit_skb",
        "saddr_v6",
    ),
    (tp::SEND_RST_SADDR_V6, "tcp", "tcp_send_reset", "saddr_v6"),
    (
        tp::RECV_RST_SADDR_V6,
        "tcp",
        "tcp_receive_reset",
        "saddr_v6",
    ),
];

/// Tracepoints attached by machina-bpfd: (program, category, event).
pub const TRACEPOINTS: &[(&str, &str, &str)] = &[
    ("mn_tp_exec", "sched", "sched_process_exec"),
    ("mn_tp_exit", "sched", "sched_process_exit"),
    ("mn_tp_fork", "sched", "sched_process_fork"),
    ("mn_tp_openat", "syscalls", "sys_enter_openat"),
    ("mn_tp_sock_state", "sock", "inet_sock_set_state"),
    ("mn_tp_kfree_skb", "skb", "kfree_skb"),
    ("mn_tp_tcp_retransmit", "tcp", "tcp_retransmit_skb"),
    ("mn_tp_tcp_send_reset", "tcp", "tcp_send_reset"),
    ("mn_tp_tcp_receive_reset", "tcp", "tcp_receive_reset"),
];

/// Resolve every TP_OFF slot; missing fields become `tp::MISSING`.
pub fn resolve_offsets(root: &Path) -> Vec<u32> {
    let mut cache: HashMap<(String, String), HashMap<String, Field>> = HashMap::new();
    let mut out = vec![tp::MISSING; tp::COUNT as usize];
    for (idx, cat, ev, field) in OFFSETS {
        let fields = cache
            .entry((cat.to_string(), ev.to_string()))
            .or_insert_with(|| {
                read_event(root, cat, ev)
                    .map(|t| parse_format(&t))
                    .unwrap_or_default()
            });
        if let Some(f) = fields.get(*field) {
            out[*idx as usize] = f.offset;
        }
    }
    out
}

pub fn drop_reason_names(root: &Path) -> HashMap<u32, String> {
    read_event(root, "skb", "kfree_skb")
        .map(|t| parse_symbolic(&t))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXEC: &str = "name: sched_process_exec\nID: 315\nformat:\n\tfield:unsigned short common_type;\toffset:0;\tsize:2;\tsigned:0;\n\tfield:__data_loc char[] filename;\toffset:8;\tsize:4;\tsigned:0;\n\tfield:pid_t pid;\toffset:12;\tsize:4;\tsigned:1;\n\tfield:__u8 saddr_v6[16];\toffset:40;\tsize:16;\tsigned:0;\n\nprint fmt: \"filename=%s\", __get_str(filename)\n";

    #[test]
    fn parses_fields() {
        let f = parse_format(EXEC);
        assert_eq!(f["filename"], Field { offset: 8, size: 4 });
        assert_eq!(f["pid"].offset, 12);
        assert_eq!(f["saddr_v6"].size, 16);
    }

    #[test]
    fn parses_symbolic_tables() {
        let t = "print fmt: \"reason: %s\", __print_symbolic(REC->reason, { 2, \"NOT_SPECIFIED\" }, { 3, \"NO_SOCKET\" }, { 0x10, \"HEX\" })";
        let m = parse_symbolic(t);
        assert_eq!(m[&2], "NOT_SPECIFIED");
        assert_eq!(m[&3], "NO_SOCKET");
        assert_eq!(m[&16], "HEX");
    }
}
