// machina: helpers for the lenient decoding patches (see MACHINA_PATCHES.md in the crate root).

use crate::{PgTypeInfo, PgValueFormat, PgValueRef};
use byteorder::{BigEndian, ByteOrder};
#[cfg(feature = "chrono")]
use chrono::{DateTime, NaiveDateTime};

pub(crate) fn is_int(ty: &PgTypeInfo) -> bool {
    *ty == PgTypeInfo::INT2 || *ty == PgTypeInfo::INT4 || *ty == PgTypeInfo::INT8
}

pub(crate) fn is_float(ty: &PgTypeInfo) -> bool {
    *ty == PgTypeInfo::FLOAT4 || *ty == PgTypeInfo::FLOAT8
}

/// Value of a binary NUMERIC as f64 (what SQLite returns for AVG(), SUM() of reals, ...).
pub(crate) fn numeric_to_f64(buf: &[u8]) -> Result<f64, String> {
    if buf.len() < 8 {
        return Err("short NUMERIC value".into());
    }
    let ndigits = BigEndian::read_i16(&buf[0..2]) as usize;
    let weight = BigEndian::read_i16(&buf[2..4]) as i32;
    let sign = BigEndian::read_u16(&buf[4..6]);
    if sign == 0xC000 {
        return Ok(f64::NAN);
    }
    let mut v = 0f64;
    for i in 0..ndigits {
        let d = BigEndian::read_i16(&buf[8 + i * 2..10 + i * 2]) as f64;
        v += d * 10_000f64.powi(weight - i as i32);
    }
    Ok(if sign == 0x4000 { -v } else { v })
}

/// Value of a binary NUMERIC as i64 (SUM() of integers is NUMERIC in PostgreSQL, an integer in SQLite). Fractions are truncated.
pub(crate) fn numeric_to_i64(buf: &[u8]) -> Result<i64, String> {
    let f = numeric_to_f64(buf)?;
    if !f.is_finite() || f.abs() >= 9.2e18 {
        return Err(format!("NUMERIC {f} does not fit in i64"));
    }
    Ok(f as i64)
}

#[cfg(feature = "chrono")]
/// A timestamp stored as TEXT: RFC 3339 (any offset, converted to UTC), `YYYY-MM-DD HH:MM:SS[.f]` (UTC), the ISO form with `T`
/// and a trailing `Z`, optionally with a `+00` style suffix.
pub(crate) fn parse_text_timestamp(s: &str) -> Result<NaiveDateTime, String> {
    let s = s.trim();
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Ok(dt.naive_utc());
    }
    let s2 = s.trim_end_matches('Z');
    for fmt in ["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%d %H:%M:%S%.f"] {
        if let Ok(n) = NaiveDateTime::parse_from_str(s2, fmt) {
            return Ok(n);
        }
    }
    for fmt in ["%Y-%m-%d %H:%M:%S%.f%#z", "%Y-%m-%dT%H:%M:%S%.f%#z"] {
        if let Ok(dt) = DateTime::parse_from_str(s, fmt) {
            return Ok(dt.naive_utc());
        }
    }
    Err(format!("cannot read {s:?} as a timestamp"))
}

/// The value as UTF-8 text when its column is TEXT-like (binary and text format carry the same bytes).
pub(crate) fn text_of<'a>(value: &'a PgValueRef<'_>) -> Option<Result<&'a str, std::str::Utf8Error>> {
    if value.type_info == PgTypeInfo::TEXT || value.type_info == PgTypeInfo::VARCHAR {
        let bytes = match value.format() {
            PgValueFormat::Binary | PgValueFormat::Text => value.as_bytes().ok()?,
        };
        Some(std::str::from_utf8(bytes))
    } else {
        None
    }
}
