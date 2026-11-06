//! Catalog bytes: path to slot id, with a CRC so a torn write fails closed.

use std::collections::BTreeMap;

use relate::Error;

use super::path::check_path;
use super::slot::SyncSlot;

const CATALOG_MAGIC: &[u8] = b"MDBOPF01";
const CATALOG_MAX: u64 = 1024 * 1024;

pub(crate) struct Inner {
    pub(crate) catalog: Box<dyn SyncSlot>,
    pub(crate) slots: Vec<Box<dyn SyncSlot>>,
    pub(crate) entries: BTreeMap<String, u32>,
    pub(crate) freelist: Vec<u32>,
}

impl Inner {
    pub(crate) fn publish(&mut self) -> relate::Result<()> {
        self.write_catalog(&self.entries)
    }

    pub(crate) fn commit(
        &mut self,
        entries: BTreeMap<String, u32>,
        freelist: Vec<u32>,
    ) -> relate::Result<()> {
        self.write_catalog(&entries)?;
        self.entries = entries;
        self.freelist = freelist;
        Ok(())
    }

    fn write_catalog(&self, entries: &BTreeMap<String, u32>) -> relate::Result<()> {
        let bytes = encode_catalog(entries)?;
        self.catalog.write_at(0, &bytes)?;
        self.catalog.truncate(bytes.len() as u64)?;
        self.catalog
            .flush()
            .map_err(|e| Error::storage(format!("catalog flush failed: {e}")))
    }

    pub(crate) fn alloc(&mut self, path: &str) -> relate::Result<()> {
        let Some(&slot) = self.freelist.last() else {
            return Err(Error::storage("OPFS slot pool exhausted"));
        };
        self.slots[slot as usize].truncate(0)?;
        self.slots[slot as usize].flush()?;
        let mut entries = self.entries.clone();
        entries.insert(path.to_string(), slot);
        let mut freelist = self.freelist.clone();
        freelist.pop();
        self.commit(entries, freelist)
    }

    pub(crate) fn require(&self, path: &str) -> relate::Result<u32> {
        self.entries
            .get(path)
            .copied()
            .ok_or_else(|| Error::io(format!("missing {path}")))
    }
}

fn encode_catalog(entries: &BTreeMap<String, u32>) -> relate::Result<Vec<u8>> {
    let mut body = Vec::new();
    body.extend_from_slice(CATALOG_MAGIC);
    body.extend_from_slice(&1u32.to_le_bytes());
    body.extend_from_slice(&(entries.len() as u32).to_le_bytes());
    for (path, slot) in entries {
        let path = path.as_bytes();
        body.extend_from_slice(&slot.to_le_bytes());
        body.extend_from_slice(&(path.len() as u32).to_le_bytes());
        body.extend_from_slice(path);
    }
    let crc = crc32fast::hash(&body);
    body.extend_from_slice(&crc.to_le_bytes());
    Ok(body)
}

pub(crate) fn read_catalog(catalog: &dyn SyncSlot) -> relate::Result<BTreeMap<String, u32>> {
    let len = catalog.size()?;
    if len == 0 {
        return Ok(BTreeMap::new());
    }
    if len > CATALOG_MAX || len < CATALOG_MAGIC.len() as u64 + 12 {
        return Err(Error::storage("corrupt OPFS catalog"));
    }
    let mut buf = vec![0u8; len as usize];
    let mut off = 0;
    while off < buf.len() {
        let n = catalog.read_at(off as u64, &mut buf[off..])?;
        if n == 0 {
            return Err(Error::storage("corrupt OPFS catalog"));
        }
        off += n;
    }
    decode_catalog(&buf)
}

fn decode_catalog(buf: &[u8]) -> relate::Result<BTreeMap<String, u32>> {
    if buf.len() < CATALOG_MAGIC.len() + 12 || !buf.starts_with(CATALOG_MAGIC) {
        return Err(Error::storage("corrupt OPFS catalog"));
    }
    let (body, crc_bytes) = buf.split_at(buf.len() - 4);
    let crc = u32::from_le_bytes(crc_bytes.try_into().expect("4 bytes"));
    if crc32fast::hash(body) != crc {
        return Err(Error::storage("corrupt OPFS catalog"));
    }
    let mut rest = &body[CATALOG_MAGIC.len()..];
    let version = take_u32(&mut rest)?;
    if version != 1 {
        return Err(Error::storage(format!(
            "unsupported OPFS catalog version {version}"
        )));
    }
    let count = take_u32(&mut rest)?;
    let mut entries = BTreeMap::new();
    for _ in 0..count {
        let slot = take_u32(&mut rest)?;
        let path_len = take_u32(&mut rest)? as usize;
        if rest.len() < path_len {
            return Err(Error::storage("corrupt OPFS catalog"));
        }
        let (path_bytes, next) = rest.split_at(path_len);
        rest = next;
        let path =
            std::str::from_utf8(path_bytes).map_err(|_| Error::storage("corrupt OPFS catalog"))?;
        check_path(path)?;
        if entries.insert(path.to_string(), slot).is_some() {
            return Err(Error::storage("corrupt OPFS catalog"));
        }
    }
    if !rest.is_empty() {
        return Err(Error::storage("corrupt OPFS catalog"));
    }
    Ok(entries)
}

fn take_u32(rest: &mut &[u8]) -> relate::Result<u32> {
    if rest.len() < 4 {
        return Err(Error::storage("corrupt OPFS catalog"));
    }
    let (bytes, next) = rest.split_at(4);
    *rest = next;
    Ok(u32::from_le_bytes(bytes.try_into().expect("4 bytes")))
}
