use lsm_tree::Guard;

/// Convert an iterator guard into owned `(key, value)` byte vectors.
///
/// Returns `Err` on I/O failure (e.g. BlobTree indirection read) instead
/// of panicking, so property tests get a clear error message.
pub fn guard_to_kv(guard: impl Guard) -> lsm_tree::Result<(Vec<u8>, Vec<u8>)> {
    let (k, v) = guard.into_inner()?;
    Ok((k.to_vec(), v.to_vec()))
}
