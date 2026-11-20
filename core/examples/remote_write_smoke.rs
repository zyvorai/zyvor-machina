// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Write a Snappy-compressed prometheus.WriteRequest to stdout (for curl tests).

use std::io::{self, Write};

fn main() {
    let req = machina_core::write_request_with_gauge(
        "machina_host_cpu_percent",
        42.0,
        chrono::Utc::now().timestamp_millis(),
    );
    let body = machina_core::encode_remote_write_body(&req).expect("encode");
    io::stdout().write_all(&body).expect("stdout");
}
