use super::Cache;

/// A cached row must own its value, not view the data block the value was read
/// out of.
///
/// The point-read path produces a value as a subslice of the decoded block, and
/// a subslice keeps the whole block allocation alive. The weigher charges the
/// row only its own key and value bytes, so a hundred-byte row viewing a four
/// kilobyte block is accounted as a hundred bytes while holding four thousand.
/// A workload touching one key per block would then exceed the capacity it
/// asked for by the ratio between the two, which is exactly the workload a
/// row cache is least able to help.
///
/// The observable is the address: a view's bytes live inside the block's
/// buffer, a copy's do not. That holds for either slice backend, where a
/// reference count does not.
#[test]
fn row_cache_when_given_a_block_subslice_stores_a_detached_copy() {
    let cache = Cache::with_capacity_bytes(1024 * 1024);
    let id = crate::table::GlobalTableId::from((0, 0));

    // Stand in for a decoded data block, large enough that the slice is
    // heap-backed rather than stored inline.
    let block = crate::Slice::from(vec![7_u8; 4096]);
    let block_range = {
        let start = block.as_ptr() as usize;
        start..start + block.len()
    };

    let value = block.slice(0..8);
    assert!(
        block_range.contains(&(value.as_ptr() as usize)),
        "precondition: a subslice points into the block's own buffer",
    );

    cache.insert_row(
        id,
        1,
        crate::InternalValue {
            key: crate::key::InternalKey::new(
                crate::UserKey::from(&b"k"[..]),
                1,
                crate::ValueType::Value,
            ),
            value,
        },
    );

    let Some(got) = cache.get_row(id, 1, b"k") else {
        panic!("the row was just inserted, so the lookup must hit");
    };

    assert!(
        !block_range.contains(&(got.value.as_ptr() as usize)),
        "the cached row still points into the block it was read from, so it \
         keeps the whole block alive while being charged only its own bytes",
    );

    // And the copy has to be a faithful one.
    assert_eq!(&*got.value, &[7_u8; 8][..]);
}

#[test]
fn metadata_priority_defaults_on_and_toggles() {
    // On by default.
    assert!(Cache::with_capacity_bytes(1024).metadata_priority());
    // Builder turns it off and back on.
    let off = Cache::with_capacity_bytes(1024).with_metadata_priority(false);
    assert!(!off.metadata_priority());
    assert!(off.with_metadata_priority(true).metadata_priority());
}

/// A blob cache entry is keyed by a POSITION in a blob file, and a corrupt
/// index entry can point a second key at a position that already holds another
/// key's value. A direct read catches that: the reader compares the key it was
/// asked for against the one stored in the record, and refuses the mismatch.
/// The cached path has to reach the same verdict, or a corrupt entry served
/// from cache would silently return the neighbouring key's value instead of
/// reporting the corruption.
#[test]
fn a_blob_lookup_under_a_conflicting_key_misses_rather_than_serving_the_other_value() {
    use crate::vlog::ValueHandle;

    let cache = Cache::with_capacity_bytes(1024 * 1024);
    let vhandle = ValueHandle {
        blob_file_id: 7,
        offset: 4096,
        on_disk_size: 5,
    };

    cache.insert_blob(
        0,
        &vhandle,
        b"real-key",
        crate::UserValue::from(&b"value"[..]),
    );

    // The key it was stored under still finds it.
    assert_eq!(
        cache.get_blob(0, &vhandle, b"real-key").as_deref(),
        Some(&b"value"[..]),
        "the owning key must still hit",
    );

    // A different key pointed at the same position must NOT be served this
    // value; it reads as a miss so the caller does the real read and gets the
    // real error.
    assert!(
        cache.get_blob(0, &vhandle, b"other-key").is_none(),
        "a conflicting key must not be served the value at that offset",
    );
}

/// The key is not the only thing a direct read checks: `Reader::parse_record`
/// also rejects a handle whose declared `on_disk_size` disagrees with the size
/// in the record header. A corrupt handle can name the right key at the right
/// offset and still be wrong about the size, so the cached path has to compare
/// that too or it serves a value the reader would have refused.
#[test]
fn a_blob_lookup_with_a_conflicting_size_misses_rather_than_serving_the_value() {
    use crate::vlog::ValueHandle;

    let cache = Cache::with_capacity_bytes(1024 * 1024);
    let stored = ValueHandle {
        blob_file_id: 7,
        offset: 4096,
        on_disk_size: 5,
    };

    cache.insert_blob(0, &stored, b"key", crate::UserValue::from(&b"value"[..]));

    assert_eq!(
        cache.get_blob(0, &stored, b"key").as_deref(),
        Some(&b"value"[..]),
        "the handle it was stored under must still hit",
    );

    // Same file, same offset, same key, different declared size: the reader
    // would reject this handle, so the cache must not answer for it.
    let conflicting = ValueHandle {
        on_disk_size: 9,
        ..stored
    };
    assert!(
        cache.get_blob(0, &conflicting, b"key").is_none(),
        "a handle declaring a different size must not be served the cached value",
    );
}
