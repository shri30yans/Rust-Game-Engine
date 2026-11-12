//! Deterministic, validated content packs shared by desktop, Android and headless startup.
use std::collections::BTreeMap;
use std::io::{Read, Seek, SeekFrom, Write};
use std::sync::Arc;

use thiserror::Error;

use crate::path::{AssetPath, InvalidContentName};

const MAGIC: &[u8; 8] = b"ENGPAK01";
const VERSION: u16 = 1;
const HEADER_LEN: usize = 10;
const COUNT_OFFSET: u64 = 14;
const HASH_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const HASH_PRIME: u64 = 0x0000_0100_0000_01b3;

/// A validated entry in a mounted game content pack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackEntry {
    /// Logical key for the entry.
    pub path: AssetPath,
    /// Integrity hash of the entry bytes.
    pub hash: u64,
    /// Content bytes.
    pub bytes: Vec<u8>,
}

/// Mounted immutable pack, indexed by logical asset path.
#[derive(Debug, Clone, Default)]
pub struct ContentPack {
    entries: Arc<BTreeMap<AssetPath, PackEntry>>,
}

/// Decodes the shared packed-content format from any platform-provided seekable stream.
pub struct PackReader;

/// Failure while reading, validating or encoding a content pack.
#[derive(Debug, Error)]
pub enum PackError {
    /// Underlying pack stream failed.
    #[error("content pack I/O failed: {0}")]
    Io(#[from] std::io::Error),
    /// Pack header or table is malformed.
    #[error("invalid content pack header or table")]
    Header,
    /// Pack version is not supported.
    #[error("unsupported content pack version {0}")]
    Version(u16),
    /// An entry has an invalid logical path.
    #[error("invalid asset path in content pack")]
    Path(#[from] InvalidContentName),
    /// Entry data does not match its table hash.
    #[error("content hash mismatch for {0}")]
    Integrity(AssetPath),
    /// Requested path does not exist in this pack.
    #[error("asset not found in content pack: {0}")]
    Missing(AssetPath),
    /// Pack table contains a duplicate path or invalid range.
    #[error("duplicate path or invalid range in content pack")]
    Structure,
    /// Encoder received the same logical path more than once.
    #[error("duplicate path while writing content pack: {0}")]
    Duplicate(AssetPath),
}

impl PackReader {
    /// Reads and validates every table entry and its bytes from a seekable pack source.
    pub fn read<R: Read + Seek>(mut reader: R) -> Result<ContentPack, PackError> {
        let mut header = [0; HEADER_LEN];
        reader.read_exact(&mut header)?;
        if header.get(..8) != Some(MAGIC.as_slice()) {
            return Err(PackError::Header);
        }
        let version = u16::from_le_bytes(
            header
                .get(8..10)
                .ok_or(PackError::Header)?
                .try_into()
                .map_err(|_| PackError::Header)?,
        );
        if version != VERSION {
            return Err(PackError::Version(version));
        }
        let mut count_bytes = [0; 4];
        reader.read_exact(&mut count_bytes)?;
        let count = u32::from_le_bytes(count_bytes);
        let count = usize::try_from(count).map_err(|_| PackError::Structure)?;
        let total_len = reader.seek(SeekFrom::End(0))?;
        reader.seek(SeekFrom::Start(COUNT_OFFSET))?;
        if u64::try_from(count).map_err(|_| PackError::Structure)?
            > total_len.saturating_sub(COUNT_OFFSET) / 26
        {
            return Err(PackError::Structure);
        }
        let mut table = Vec::with_capacity(count);
        for _ in 0..count {
            let mut len = [0; 2];
            reader.read_exact(&mut len)?;
            let mut path_bytes = vec![0; usize::from(u16::from_le_bytes(len))];
            reader.read_exact(&mut path_bytes)?;
            let path = String::from_utf8(path_bytes).map_err(|_| PackError::Structure)?;
            let path = AssetPath::new(path)?;
            let mut fields = [0; 24];
            reader.read_exact(&mut fields)?;
            let offset =
                u64::from_le_bytes(fields[0..8].try_into().map_err(|_| PackError::Structure)?);
            let size =
                u64::from_le_bytes(fields[8..16].try_into().map_err(|_| PackError::Structure)?);
            let hash = u64::from_le_bytes(
                fields[16..24]
                    .try_into()
                    .map_err(|_| PackError::Structure)?,
            );
            table.push((path, offset, size, hash));
        }
        let table_end = reader.stream_position()?;
        let mut ranges = table
            .iter()
            .map(|(_, offset, size, _)| {
                let end = offset.checked_add(*size).ok_or(PackError::Structure)?;
                if *offset < table_end || end > total_len {
                    return Err(PackError::Structure);
                }
                Ok((*offset, end))
            })
            .collect::<Result<Vec<_>, PackError>>()?;
        ranges.sort_unstable();
        if ranges.windows(2).any(|pair| {
            pair.first()
                .zip(pair.get(1))
                .is_some_and(|(previous, next)| previous.1 > next.0)
        }) {
            return Err(PackError::Structure);
        }
        let mut entries = BTreeMap::new();
        for (path, offset, size, hash) in table {
            if offset < table_end || entries.contains_key(&path) {
                return Err(PackError::Structure);
            }
            let size = usize::try_from(size).map_err(|_| PackError::Structure)?;
            reader.seek(SeekFrom::Start(offset))?;
            let mut bytes = vec![0; size];
            reader.read_exact(&mut bytes)?;
            if stable_hash(&bytes) != hash {
                return Err(PackError::Integrity(path));
            }
            entries.insert(path.clone(), PackEntry { path, hash, bytes });
        }
        Ok(ContentPack {
            entries: Arc::new(entries),
        })
    }
}

impl ContentPack {
    /// Returns checked bytes for a logical asset path.
    pub fn load(&self, path: &AssetPath) -> Result<&[u8], PackError> {
        self.entries
            .get(path)
            .map(|entry| entry.bytes.as_slice())
            .ok_or_else(|| PackError::Missing(path.clone()))
    }

    /// Reads a UTF-8 text entry by logical path.
    pub fn load_text(&self, path: &AssetPath) -> Result<&str, PackError> {
        std::str::from_utf8(self.load(path)?).map_err(|_| PackError::Structure)
    }

    /// Returns the entry's stable content hash.
    pub fn hash(&self, path: &AssetPath) -> Result<u64, PackError> {
        self.entries
            .get(path)
            .map(|entry| entry.hash)
            .ok_or_else(|| PackError::Missing(path.clone()))
    }

    /// Returns all mounted entries in stable logical-path order.
    pub fn entries(&self) -> impl Iterator<Item = &PackEntry> {
        self.entries.values()
    }

    /// Encodes entries with a deterministic table and content hashes.
    pub fn write<W: Write>(
        entries: impl IntoIterator<Item = (AssetPath, Vec<u8>)>,
        mut writer: W,
    ) -> Result<(), PackError> {
        let mut indexed = BTreeMap::new();
        for (path, bytes) in entries {
            if indexed.insert(path.clone(), bytes).is_some() {
                return Err(PackError::Duplicate(path));
            }
        }
        let entries = indexed;
        let table_len = entries.values().try_fold(14_u64, |total, _| {
            total.checked_add(24).ok_or(PackError::Structure)
        })?;
        let names_len = entries.keys().try_fold(0_u64, |total, path| {
            let length = u16::try_from(path.as_str().len()).map_err(|_| PackError::Structure)?;
            total
                .checked_add(2 + u64::from(length))
                .ok_or(PackError::Structure)
        })?;
        let data_start = table_len
            .checked_add(names_len)
            .ok_or(PackError::Structure)?;
        writer.write_all(MAGIC)?;
        writer.write_all(&VERSION.to_le_bytes())?;
        writer.write_all(
            &u32::try_from(entries.len())
                .map_err(|_| PackError::Structure)?
                .to_le_bytes(),
        )?;
        let mut offset = data_start;
        for (path, bytes) in &entries {
            let path_len = u16::try_from(path.as_str().len()).map_err(|_| PackError::Structure)?;
            writer.write_all(&path_len.to_le_bytes())?;
            writer.write_all(path.as_str().as_bytes())?;
            writer.write_all(&offset.to_le_bytes())?;
            let size = u64::try_from(bytes.len()).map_err(|_| PackError::Structure)?;
            writer.write_all(&size.to_le_bytes())?;
            writer.write_all(&stable_hash(bytes).to_le_bytes())?;
            offset = offset.checked_add(size).ok_or(PackError::Structure)?;
        }
        for bytes in entries.values() {
            writer.write_all(bytes)?;
        }
        Ok(())
    }
}

/// Stable FNV-1a hash used for pack entry integrity.
#[must_use]
pub fn stable_hash(bytes: &[u8]) -> u64 {
    bytes.iter().fold(HASH_OFFSET, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(HASH_PRIME)
    })
}
