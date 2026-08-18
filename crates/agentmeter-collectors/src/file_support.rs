use std::{fmt::Write as _, fs::File, io::Read, path::Path};

use sha2::{Digest, Sha256};

use crate::{CollectorError, SourceCandidate, SourceCheckpoint, SourceKind};

pub(crate) fn checkpoint_continues(
    path: &Path,
    checkpoint: &SourceCheckpoint,
    source_len: u64,
) -> Result<bool, CollectorError> {
    let Some(offset) = checkpoint.byte_offset else {
        return Ok(false);
    };
    if source_len < offset || checkpoint.source_len < offset {
        return Ok(false);
    }
    let Some(expected) = checkpoint.prefix_fingerprint.as_deref() else {
        return Ok(false);
    };
    Ok(hash_prefix(path, offset)? == expected)
}

pub(crate) fn hash_prefix(path: &Path, len: u64) -> Result<String, CollectorError> {
    hash_reader(File::open(path).map_err(io_error)?.take(len))
}

pub(crate) fn hash_file(path: &Path) -> Result<String, CollectorError> {
    hash_reader(File::open(path).map_err(io_error)?)
}

pub(crate) fn hash_bytes(bytes: &[u8]) -> String {
    digest_hex(Sha256::digest(bytes))
}

pub(crate) fn ensure_kind(
    source: &SourceCandidate,
    expected: SourceKind,
) -> Result<(), CollectorError> {
    if source.kind == expected {
        Ok(())
    } else {
        Err(CollectorError::new(format!(
            "source kind {:?} is not {:?}",
            source.kind, expected
        )))
    }
}

pub(crate) fn io_error(error: std::io::Error) -> CollectorError {
    CollectorError::new(error.to_string())
}

fn hash_reader(mut reader: impl Read) -> Result<String, CollectorError> {
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 8192];
    loop {
        let bytes_read = reader.read(&mut buffer).map_err(io_error)?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }
    Ok(digest_hex(hasher.finalize()))
}

fn digest_hex(digest: impl AsRef<[u8]>) -> String {
    digest
        .as_ref()
        .iter()
        .fold(String::with_capacity(64), |mut encoded, byte| {
            write!(encoded, "{byte:02x}").expect("writing to a String cannot fail");
            encoded
        })
}
