use super::*;
use crate::TreeType;

fn empty_version(id: u64) -> Version {
    Version::new(id, TreeType::Standard)
}

#[test]
fn set_retention_floor_below_the_current_one_is_a_no_op() {
    let mut v = empty_version(1).with_retention_floor(40);
    v.set_retention_floor(10);
    assert_eq!(v.retention_floor(), 40, "the floor never goes down");
    v.set_retention_floor(40);
    assert_eq!(v.retention_floor(), 40, "equal is not above");
}

#[test]
fn set_retention_floor_on_a_sole_owner_writes_through() {
    let mut v = empty_version(1);
    let before = Arc::as_ptr(&v.inner);

    v.set_retention_floor(40);

    assert_eq!(v.retention_floor(), 40);
    assert!(
        core::ptr::eq(before, Arc::as_ptr(&v.inner)),
        "a sole owner keeps its allocation instead of rebuilding it",
    );
}

#[test]
fn set_retention_floor_on_a_shared_handle_leaves_the_other_owner_alone() {
    let mut v = empty_version(1);
    // A second handle on the same allocation, which is what an install whose
    // mutator returned the prior version untouched holds.
    let shared = v.clone();

    v.set_retention_floor(40);

    assert_eq!(
        v.retention_floor(),
        40,
        "the raised handle carries the floor"
    );
    assert_eq!(
        shared.retention_floor(),
        0,
        "the other owner must not see a floor it never recorded",
    );
    assert!(
        !core::ptr::eq(Arc::as_ptr(&v.inner), Arc::as_ptr(&shared.inner)),
        "a shared handle rebuilds rather than mutating in place",
    );
}
