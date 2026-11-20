// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

const BYTES_PER_KB: f64 = 1024.0;
const BYTES_PER_MB: f64 = 1_048_576.0;
const BYTES_PER_GB: f64 = 1_073_741_824.0;

pub fn format_bytes(bytes: u64) -> String {
    let b = bytes as f64;
    if b >= BYTES_PER_GB {
        format!("{:.1} GB", b / BYTES_PER_GB)
    } else if b >= BYTES_PER_MB {
        format!("{:.1} MB", b / BYTES_PER_MB)
    } else if b >= BYTES_PER_KB {
        format!("{:.1} KB", b / BYTES_PER_KB)
    } else {
        format!("{bytes} B")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1024), "1.0 KB");
        assert_eq!(format_bytes(1_048_576), "1.0 MB");
        assert_eq!(format_bytes(1_073_741_824), "1.0 GB");
        assert_eq!(format_bytes(2_684_354_560), "2.5 GB");
    }
}
