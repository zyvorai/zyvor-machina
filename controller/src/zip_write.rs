// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Minimal write-only ZIP (deflate). No archive extraction — we never take
//! untrusted zip input, so we avoid the `zip` crate's parser surface.

use std::io::{self, Write};

use crc32fast::Hasher;
use flate2::write::DeflateEncoder;
use flate2::Compression;

struct Entry {
    name: String,
    method: u16,
    crc: u32,
    compressed_len: u32,
    uncompressed_len: u32,
    local_header_offset: u32,
}

/// Build a ZIP from `(path, body)` pairs using deflate compression.
pub fn deflated_zip(files: impl IntoIterator<Item = (String, Vec<u8>)>) -> io::Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut entries = Vec::new();

    for (name, data) in files {
        validate_entry_name(&name)?;
        let local_header_offset = u32::try_from(out.len()).map_err(|_| too_large())?;

        let mut hasher = Hasher::new();
        hasher.update(&data);
        let crc = hasher.finalize();
        let uncompressed_len = u32::try_from(data.len()).map_err(|_| too_large())?;

        let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&data)?;
        let compressed = encoder.finish()?;
        let compressed_len = u32::try_from(compressed.len()).map_err(|_| too_large())?;
        let method: u16 = 8; // deflate

        write_local_header(
            &mut out,
            &name,
            method,
            crc,
            compressed_len,
            uncompressed_len,
        )?;
        out.write_all(&compressed)?;

        entries.push(Entry {
            name,
            method,
            crc,
            compressed_len,
            uncompressed_len,
            local_header_offset,
        });
    }

    let central_dir_offset = u32::try_from(out.len()).map_err(|_| too_large())?;
    for e in &entries {
        write_central_header(&mut out, e)?;
    }
    let central_dir_len =
        u32::try_from(out.len()).map_err(|_| too_large())? - central_dir_offset;
    let entry_count = u16::try_from(entries.len()).map_err(|_| {
        io::Error::new(io::ErrorKind::InvalidInput, "too many zip entries")
    })?;

    // End of central directory.
    out.write_all(&0x06054b50u32.to_le_bytes())?;
    out.write_all(&0u16.to_le_bytes())?; // disk number
    out.write_all(&0u16.to_le_bytes())?; // disk with central dir
    out.write_all(&entry_count.to_le_bytes())?;
    out.write_all(&entry_count.to_le_bytes())?;
    out.write_all(&central_dir_len.to_le_bytes())?;
    out.write_all(&central_dir_offset.to_le_bytes())?;
    out.write_all(&0u16.to_le_bytes())?; // comment len
    Ok(out)
}

fn validate_entry_name(name: &str) -> io::Result<()> {
    if name.is_empty()
        || name.starts_with('/')
        || name.contains('\\')
        || name.contains('\0')
        || name.split('/').any(|p| p == "..")
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "zip entry path must be a relative path without '..'",
        ));
    }
    Ok(())
}

fn too_large() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, "zip larger than 4 GiB")
}

fn write_local_header(
    out: &mut Vec<u8>,
    name: &str,
    method: u16,
    crc: u32,
    compressed_len: u32,
    uncompressed_len: u32,
) -> io::Result<()> {
    let name_bytes = name.as_bytes();
    let name_len = u16::try_from(name_bytes.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "zip entry name too long"))?;
    out.write_all(&0x04034b50u32.to_le_bytes())?;
    out.write_all(&20u16.to_le_bytes())?;
    out.write_all(&0u16.to_le_bytes())?;
    out.write_all(&method.to_le_bytes())?;
    out.write_all(&0u16.to_le_bytes())?;
    out.write_all(&0u16.to_le_bytes())?;
    out.write_all(&crc.to_le_bytes())?;
    out.write_all(&compressed_len.to_le_bytes())?;
    out.write_all(&uncompressed_len.to_le_bytes())?;
    out.write_all(&name_len.to_le_bytes())?;
    out.write_all(&0u16.to_le_bytes())?;
    out.write_all(name_bytes)?;
    Ok(())
}

fn write_central_header(out: &mut Vec<u8>, e: &Entry) -> io::Result<()> {
    let name_bytes = e.name.as_bytes();
    let name_len = u16::try_from(name_bytes.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "zip entry name too long"))?;
    out.write_all(&0x02014b50u32.to_le_bytes())?;
    out.write_all(&20u16.to_le_bytes())?;
    out.write_all(&20u16.to_le_bytes())?;
    out.write_all(&0u16.to_le_bytes())?;
    out.write_all(&e.method.to_le_bytes())?;
    out.write_all(&0u16.to_le_bytes())?;
    out.write_all(&0u16.to_le_bytes())?;
    out.write_all(&e.crc.to_le_bytes())?;
    out.write_all(&e.compressed_len.to_le_bytes())?;
    out.write_all(&e.uncompressed_len.to_le_bytes())?;
    out.write_all(&name_len.to_le_bytes())?;
    out.write_all(&0u16.to_le_bytes())?;
    out.write_all(&0u16.to_le_bytes())?;
    out.write_all(&0u16.to_le_bytes())?;
    out.write_all(&0u16.to_le_bytes())?;
    out.write_all(&0u32.to_le_bytes())?;
    out.write_all(&e.local_header_offset.to_le_bytes())?;
    out.write_all(name_bytes)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_signatures() {
        let bytes = deflated_zip([(
            "hello.txt".into(),
            b"hello machina".to_vec(),
        )])
        .unwrap();
        assert_eq!(&bytes[..4], &0x04034b50u32.to_le_bytes());
        assert!(bytes.windows(4).any(|w| w == 0x06054b50u32.to_le_bytes()));
        assert!(validate_entry_name("../etc/passwd").is_err());
    }
}
