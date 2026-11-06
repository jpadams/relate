//! Fake-handle persistence tests. They run under Node; the browser smoke
//! covers the real sync-access handles.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use relate::{Database, Error, OpenOptions, Value};
use relate_storage::{Vfs, VfsOpen};
use wasm_bindgen_test::wasm_bindgen_test;

use super::slot::SyncSlot;
use super::vfs::OpfsVfs;

struct MemSlot {
    data: Mutex<Vec<u8>>,
    fail_flush: AtomicBool,
}

impl MemSlot {
    fn new() -> Self {
        Self {
            data: Mutex::new(Vec::new()),
            fail_flush: AtomicBool::new(false),
        }
    }

    fn fail_flush(&self) {
        self.fail_flush.store(true, Ordering::Relaxed);
    }
}

impl SyncSlot for MemSlot {
    fn read_at(&self, at: u64, buf: &mut [u8]) -> relate::Result<usize> {
        let data = self.data.lock().unwrap_or_else(|e| e.into_inner());
        let start = at as usize;
        if start >= data.len() || buf.is_empty() {
            return Ok(0);
        }
        let n = buf.len().min(data.len() - start);
        buf[..n].copy_from_slice(&data[start..start + n]);
        Ok(n)
    }

    fn write_at(&self, at: u64, buf: &[u8]) -> relate::Result<()> {
        let mut data = self.data.lock().unwrap_or_else(|e| e.into_inner());
        let start = at as usize;
        if start > data.len() {
            data.resize(start, 0);
        }
        let end = start + buf.len();
        if end > data.len() {
            data.resize(end, 0);
        }
        data[start..end].copy_from_slice(buf);
        Ok(())
    }

    fn flush(&self) -> relate::Result<()> {
        if self.fail_flush.load(Ordering::Relaxed) {
            Err(Error::storage("flush failed"))
        } else {
            Ok(())
        }
    }

    fn size(&self) -> relate::Result<u64> {
        Ok(self.data.lock().unwrap_or_else(|e| e.into_inner()).len() as u64)
    }

    fn truncate(&self, len: u64) -> relate::Result<()> {
        self.data
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .resize(len as usize, 0);
        Ok(())
    }
}

struct Pool {
    catalog: Arc<MemSlot>,
    slots: Vec<Arc<MemSlot>>,
}

impl Pool {
    fn new(n: usize) -> Self {
        Self {
            catalog: Arc::new(MemSlot::new()),
            slots: (0..n).map(|_| Arc::new(MemSlot::new())).collect(),
        }
    }

    fn open(&self) -> OpfsVfs {
        let files = self
            .slots
            .iter()
            .map(|slot| Box::new(SharedSlot(Arc::clone(slot))) as Box<dyn SyncSlot>)
            .collect();
        OpfsVfs::open_slots(Box::new(SharedSlot(Arc::clone(&self.catalog))), files).unwrap()
    }
}

struct SharedSlot(Arc<MemSlot>);

impl SyncSlot for SharedSlot {
    fn read_at(&self, at: u64, buf: &mut [u8]) -> relate::Result<usize> {
        self.0.read_at(at, buf)
    }
    fn write_at(&self, at: u64, buf: &[u8]) -> relate::Result<()> {
        self.0.write_at(at, buf)
    }
    fn flush(&self) -> relate::Result<()> {
        self.0.flush()
    }
    fn size(&self) -> relate::Result<u64> {
        self.0.size()
    }
    fn truncate(&self, len: u64) -> relate::Result<()> {
        self.0.truncate(len)
    }
}

#[wasm_bindgen_test]
fn reopen_replays_a_committed_node() {
    let pool = Pool::new(8);
    {
        let vfs = Arc::new(pool.open());
        let db = Database::open_vfs(vfs as Arc<dyn Vfs>, OpenOptions::default()).unwrap();
        let mut conn = db.connect().unwrap();
        conn.execute("CREATE (:Kept {k: 1})").unwrap();
    }
    let vfs = Arc::new(pool.open());
    let db = Database::open_vfs(vfs as Arc<dyn Vfs>, OpenOptions::default()).unwrap();
    let mut conn = db.connect().unwrap();
    let found = conn.execute("MATCH (n:Kept) RETURN count(n) AS c").unwrap();
    assert_eq!(found.rows[0][0], Value::Int(1));
}

#[wasm_bindgen_test]
fn pool_exhaustion_is_a_storage_error() {
    let vfs = Pool::new(1).open();
    vfs.open("a", VfsOpen::create_truncate()).unwrap();
    let err = match vfs.open("b", VfsOpen::create_truncate()) {
        Ok(_) => panic!("second file should exhaust the pool"),
        Err(err) => err,
    };
    assert!(
        matches!(err, Error::Storage(ref msg) if msg.contains("slot pool exhausted")),
        "{err}"
    );
}

#[wasm_bindgen_test]
fn catalog_flush_failure_is_a_storage_error() {
    let pool = Pool::new(4);
    pool.catalog.fail_flush();
    let vfs = pool.open();
    let err = match vfs.open("a", VfsOpen::create_truncate()) {
        Ok(_) => panic!("catalog flush should fail"),
        Err(err) => err,
    };
    assert!(
        matches!(err, Error::Storage(ref msg) if msg.contains("catalog flush failed")),
        "{err}"
    );
    assert!(!vfs.exists("a").unwrap());
}
