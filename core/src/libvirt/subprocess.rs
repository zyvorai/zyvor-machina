// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Stream stdout/stderr from long-running host commands into the VM create log (web UI / SSE).
//!
//! Uses [`std::sync::mpsc::SyncSender`] so a bounded queue applies backpressure (slow consumers
//! block writers instead of growing heap without bound).

use std::io::{BufRead, BufReader, Read};
use std::process::{Command, Stdio};

use crate::LibvirtError;

/// Bounded UTF-8 log lines for VM create progress. Clone for concurrent stdout/stderr readers.
pub type VmCreateLogSink = std::sync::mpsc::SyncSender<String>;

pub fn log_line(tx: Option<&VmCreateLogSink>, label: &str, line: &str) {
    if let Some(tx) = tx {
        let _ = tx.send(format!("[{label}] {line}"));
    }
}

/// Run `cmd`. When `log` is `None`, behaves like [`Command::output`]. When set, streams each
/// stdout/stderr line (prefixed) while the process runs; returned `Output` has empty stdout/stderr
/// (content was already streamed).
pub fn run_command_streaming(
    mut cmd: Command,
    summary_line: &str,
    label: &str,
    log: Option<&VmCreateLogSink>,
) -> Result<std::process::Output, LibvirtError> {
    let program = cmd.get_program().to_string_lossy().to_string();
    if log.is_none() {
        return cmd
            .output()
            .map_err(|e| LibvirtError::Operation(format!("Failed to run {program}: {e}")));
    }
    let tx = log.unwrap();
    log_line(Some(tx), label, summary_line);

    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd
        .spawn()
        .map_err(|e| LibvirtError::Operation(format!("Failed to spawn {program}: {e}")))?;
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let tx_out = tx.clone();
    let lbl = label.to_string();
    let h_out = std::thread::spawn(move || {
        stream_reader_lines(stdout, &lbl, "stdout", &tx_out);
    });
    let tx_err = tx.clone();
    let lbl2 = label.to_string();
    let h_err = std::thread::spawn(move || {
        stream_reader_lines(stderr, &lbl2, "stderr", &tx_err);
    });
    let status = child
        .wait()
        .map_err(|e| LibvirtError::Operation(format!("Failed waiting on {program}: {e}")))?;
    if h_out.join().is_err() {
        tracing::error!(program = %program, label = %label, stream = "stdout", "log reader thread panicked");
    }
    if h_err.join().is_err() {
        tracing::error!(program = %program, label = %label, stream = "stderr", "log reader thread panicked");
    }
    Ok(std::process::Output {
        status,
        stdout: Vec::new(),
        stderr: Vec::new(),
    })
}

/// Read newline-delimited chunks as **lossy UTF-8** (invalid bytes become U+FFFD, never dropped).
fn stream_reader_lines<R: Read>(r: R, label: &str, stream: &str, tx: &VmCreateLogSink) {
    let mut reader = BufReader::new(r);
    let mut buf = Vec::new();
    loop {
        buf.clear();
        match reader.read_until(b'\n', &mut buf) {
            Ok(0) => break,
            Ok(_) => {
                while matches!(buf.last(), Some(b'\n') | Some(b'\r')) {
                    buf.pop();
                }
                let s = String::from_utf8_lossy(&buf);
                let t = s.trim_end();
                if !t.is_empty() {
                    let _ = tx.send(format!("[{label}:{stream}] {t}"));
                }
            }
            Err(e) => {
                tracing::debug!(%label, %stream, "log reader read_until: {e}");
                break;
            }
        }
    }
}
