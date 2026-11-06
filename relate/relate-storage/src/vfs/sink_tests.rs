//! The bounded-serializer family's own tests: the capped in-memory sink
//! `CappedSink` and its VFS twin `CappedVfsWriter` — one test home beside
//! the module that holds the code.

use super::{CappedSink, CappedVfsWriter, MemoryVfs, Refusal, Vfs, VfsOpen};
use relate_core::Error;
use std::io::Write as _;

#[test]
fn capped_vfs_writer_does_not_write_past_its_limit() {
    let vfs = MemoryVfs::new();
    let mut file = vfs.open("candidate", VfsOpen::create_new()).unwrap();
    {
        let mut writer = CappedVfsWriter::new(
            &mut *file,
            3,
            Refusal::Fixed(Error::storage("candidate is too large")),
        );
        assert_eq!(writer.write(b"abc").unwrap(), 3);
        assert!(writer.write(b"d").is_err());
        // The crossing surfaces the sink's own typed refusal, and the
        // bound-held state answers `None`.
        let refusal = writer
            .crossed_refusal()
            .expect("a crossed bound carries its refusal");
        assert!(refusal.to_string().contains("candidate is too large"));
    }
    assert_eq!(file.len().unwrap(), 3);
}

#[test]
fn capped_writer_bounds_capacity_across_growth_and_rejects_overflow() {
    // The one typed refusal every capped sink in this test carries; the
    // serializer-facing io abort renders the same text, and the asserted
    // envelope is derived from the refusal so the two cannot drift.
    let refusal = || Error::storage("sink exceeds its envelope");
    let envelope = refusal().to_string();

    // One bound, two checks: the 32nd write pins the byte check, the
    // pre-allocated buffer pins the capacity check — both consult the
    // same figure.
    const BOUND: usize = 31;
    {
        let mut bytes = Vec::new();
        let mut sink = CappedSink::new(&mut bytes, BOUND as u64, Refusal::Fixed(refusal()));

        let mut growths = 0;
        for size in [1, 1, 2, 4, 8, 1, 5, 1, 8] {
            let previous = sink.capacity();
            sink.write_all(&vec![b'x'; size]).unwrap();
            growths += usize::from(sink.capacity() > previous);
            assert!(sink.capacity() <= BOUND);
        }
        assert!(growths <= 6, "writes should grow geometrically");
        assert_eq!(sink.captured_len(), BOUND);
        assert!(sink
            .write(b"x")
            .unwrap_err()
            .to_string()
            .contains(envelope.as_str()));
        assert_eq!(sink.captured_len(), BOUND);
        assert!(sink.capacity() <= BOUND);
    }

    {
        let mut bytes = Vec::new();
        let mut oversized = CappedSink::new(&mut bytes, BOUND as u64, Refusal::Fixed(refusal()));
        assert!(oversized.write(&[0; 32]).is_err());
        assert_eq!(oversized.capacity(), 0);
    }

    // Also reject an already overallocated Vec without attempting to
    // shrink it or copying any bytes into it.
    let mut bytes = Vec::with_capacity(32);
    {
        let mut overallocated =
            CappedSink::new(&mut bytes, BOUND as u64, Refusal::Fixed(refusal()));
        assert!(overallocated.write(b"x").is_err());
    }
    assert!(bytes.is_empty());
}

#[test]
#[should_panic(expected = "the capped capture appends into an empty buffer")]
fn capped_sink_refuses_a_non_empty_buffer() {
    // One counting base: the core counts from zero, so a buffer that
    // already holds bytes would count them once through the core and
    // again through the cap's own length checks. Construction refuses
    // it instead of silently miscounting.
    let mut bytes = vec![b'x'];
    let _ = CappedSink::new(
        &mut bytes,
        u64::MAX,
        Refusal::Fixed(Error::storage("sink exceeds its envelope")),
    );
}
