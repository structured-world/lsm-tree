use lsm_tree::Guard;

/// Convert an iterator guard into owned `(key, value)` byte vectors.
pub fn guard_to_kv(guard: impl Guard) -> (Vec<u8>, Vec<u8>) {
    let (k, v) = guard.into_inner().expect("guard into_inner failed");
    (k.to_vec(), v.to_vec())
}
