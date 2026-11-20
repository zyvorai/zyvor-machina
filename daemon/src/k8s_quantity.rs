// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Parse Kubernetes resource.Quantity strings (`cpu`, `memory`) from Node status.

/// CPU quantity → millicores (`2500m`, `8` → 8000).
pub fn parse_cpu_to_millicores(input: &str) -> Option<i64> {
    let s = input.trim();
    if s.is_empty() {
        return None;
    }
    if let Some(rest) = s.strip_suffix('m') {
        return rest.trim().parse().ok();
    }
    let n: f64 = s.parse().ok()?;
    Some((n * 1000.0) as i64)
}

/// Memory quantity → bytes (`8150581248Ki`, `16Gi`, plain integer bytes).
pub fn parse_memory_to_bytes(input: &str) -> Option<i64> {
    let s = input.trim();
    if s.is_empty() {
        return None;
    }
    /// Binary (IEC) and decimal SI suffixes as in Kubernetes apimachinery.
    /// Literal multipliers: `f64::powi` is not const on all toolchains used in CI/install.
    const PAIRS: &[(&str, f64)] = &[
        ("Ki", 1024.0),
        ("Mi", 1_048_576.0),
        ("Gi", 1_073_741_824.0),
        ("Ti", 1_099_511_627_776.0),
        ("Pi", 1_125_899_906_842_624.0),
        ("Ei", 1_152_921_504_606_846_976.0),
        ("K", 1e3),
        ("M", 1e6),
        ("G", 1e9),
        ("T", 1e12),
        ("P", 1e15),
        ("E", 1e18),
    ];
    for (suf, mult) in PAIRS {
        if let Some(prefix) = s.strip_suffix(suf) {
            let n: f64 = prefix.parse().ok()?;
            return Some((n * mult) as i64);
        }
    }
    s.parse::<i64>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_millicores() {
        assert_eq!(parse_cpu_to_millicores("2500m"), Some(2500));
        assert_eq!(parse_cpu_to_millicores("8"), Some(8000));
        assert_eq!(parse_cpu_to_millicores("1"), Some(1000));
    }

    #[test]
    fn memory_bytes() {
        assert_eq!(parse_memory_to_bytes("16Gi"), Some(16 * 1024_i64.pow(3)));
        assert_eq!(parse_memory_to_bytes("128849018880"), Some(128849018880));
    }

    #[test]
    fn memory_ki_before_k_suffix() {
        assert_eq!(parse_memory_to_bytes("100Ki"), Some(100 * 1024));
        assert_eq!(parse_memory_to_bytes("100K"), Some(100_000));
    }

    #[test]
    fn memory_binary_and_decimal_si() {
        assert_eq!(parse_memory_to_bytes("512Ki"), Some(512 * 1024));
        assert_eq!(parse_memory_to_bytes("16G"), Some(16_000_000_000));
    }
}
