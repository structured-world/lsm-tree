// Copyright (c) 2024-present, fjall-rs
// This source code is licensed under both the Apache 2.0 and MIT License
// (found in the LICENSE-* files in the repository)

use crate::double_ended_peekable::{DoubleEndedPeekable, DoubleEndedPeekableExt};
use crate::merge_operator::MergeOperator;
use crate::{InternalValue, UserKey, UserValue, ValueType};
use std::sync::Arc;

/// Consumes a stream of KVs and emits a new stream according to MVCC and tombstone rules
///
/// This iterator is used for read operations.
pub struct MvccStream<I: DoubleEndedIterator<Item = crate::Result<InternalValue>>> {
    inner: DoubleEndedPeekable<crate::Result<InternalValue>, I>,
    merge_operator: Option<Arc<dyn MergeOperator>>,
}

impl<I: DoubleEndedIterator<Item = crate::Result<InternalValue>>> MvccStream<I> {
    /// Initializes a new multi-version-aware iterator.
    #[must_use]
    pub fn new(iter: I, merge_operator: Option<Arc<dyn MergeOperator>>) -> Self {
        Self {
            inner: iter.double_ended_peekable(),
            merge_operator,
        }
    }

    /// Collects all entries for the given key and applies the merge operator (forward).
    fn resolve_merge_forward(
        &mut self,
        head: &InternalValue,
        merge_op: &dyn MergeOperator,
    ) -> crate::Result<InternalValue> {
        let user_key = &head.key.user_key;
        let mut operands: Vec<UserValue> = vec![head.value.clone()];
        let mut base_value: Option<UserValue> = None;
        let mut found_base = false;

        // Collect remaining same-key entries
        loop {
            let Some(next) = self.inner.next_if(|kv| {
                if let Ok(kv) = kv {
                    kv.key.user_key == *user_key
                } else {
                    true
                }
            }) else {
                break;
            };

            let next = next?;

            match next.key.value_type {
                ValueType::MergeOperand => {
                    operands.push(next.value);
                }
                ValueType::Value | ValueType::Indirection => {
                    base_value = Some(next.value);
                    found_base = true;
                    break;
                }
                ValueType::Tombstone | ValueType::WeakTombstone => {
                    // Tombstone kills base
                    found_base = true;
                    break;
                }
            }
        }

        // Drain any remaining same-key entries
        if found_base {
            self.drain_key_min(user_key)?;
        }

        // Reverse to chronological order (ascending seqno)
        operands.reverse();

        let operand_refs: Vec<&[u8]> = operands.iter().map(|v| v.as_ref()).collect();
        let merged = merge_op.merge(user_key, base_value.as_deref(), &operand_refs)?;

        Ok(InternalValue::from_components(
            user_key.clone(),
            merged,
            head.key.seqno,
            ValueType::Value,
        ))
    }

    /// Resolves a single merge operand (no other entries for this key).
    fn resolve_merge_single(
        &self,
        entry: &InternalValue,
        merge_op: &dyn MergeOperator,
    ) -> crate::Result<InternalValue> {
        let operand_refs: Vec<&[u8]> = vec![entry.value.as_ref()];
        let merged = merge_op.merge(&entry.key.user_key, None, &operand_refs)?;
        Ok(InternalValue::from_components(
            entry.key.user_key.clone(),
            merged,
            entry.key.seqno,
            ValueType::Value,
        ))
    }

    /// Resolves buffered entries for reverse iteration merge.
    /// `entries` are in ascending seqno order (oldest first, as collected by next_back).
    fn resolve_merge_buffered(&self, entries: Vec<InternalValue>) -> crate::Result<InternalValue> {
        let merge_op = match &self.merge_operator {
            Some(op) => op,
            None => {
                // No merge operator — return newest entry (last in ascending order)
                return entries
                    .into_iter()
                    .last()
                    .ok_or(crate::Error::Unrecoverable);
            }
        };

        // entries are in ascending seqno order (oldest→newest)
        // The newest entry has the highest seqno — that's our result seqno
        let mut operands: Vec<UserValue> = Vec::new();
        let mut base_value: Option<UserValue> = None;
        let mut result_seqno = 0;
        let mut result_key = UserKey::empty();

        // Process in descending seqno order (newest first) to match forward merge semantics
        for entry in entries.iter().rev() {
            if result_seqno == 0 {
                result_seqno = entry.key.seqno;
                result_key = entry.key.user_key.clone();
            }

            match entry.key.value_type {
                ValueType::MergeOperand => {
                    operands.push(entry.value.clone());
                }
                ValueType::Value | ValueType::Indirection => {
                    base_value = Some(entry.value.clone());
                    break;
                }
                ValueType::Tombstone | ValueType::WeakTombstone => {
                    break;
                }
            }
        }

        // Reverse operands to chronological order (ascending seqno)
        operands.reverse();

        let operand_refs: Vec<&[u8]> = operands.iter().map(|v| v.as_ref()).collect();
        let merged = merge_op.merge(&result_key, base_value.as_deref(), &operand_refs)?;

        Ok(InternalValue::from_components(
            result_key,
            merged,
            result_seqno,
            ValueType::Value,
        ))
    }

    // Drains all entries for the given user key from the front of the iterator.
    fn drain_key_min(&mut self, key: &UserKey) -> crate::Result<()> {
        loop {
            let Some(next) = self.inner.next_if(|kv| {
                if let Ok(kv) = kv {
                    kv.key.user_key == key
                } else {
                    true
                }
            }) else {
                return Ok(());
            };

            next?;
        }
    }
}

impl<I: DoubleEndedIterator<Item = crate::Result<InternalValue>>> Iterator for MvccStream<I> {
    type Item = crate::Result<InternalValue>;

    fn next(&mut self) -> Option<Self::Item> {
        let head = fail_iter!(self.inner.next()?);

        if head.key.value_type.is_merge_operand() {
            if let Some(merge_op) = self.merge_operator.clone() {
                // Collect remaining entries for this key
                let result = self.resolve_merge_forward(&head, merge_op.as_ref());
                return Some(result);
            }
        }

        // As long as items are the same key, ignore them
        fail_iter!(self.drain_key_min(&head.key.user_key));

        Some(Ok(head))
    }
}

impl<I: DoubleEndedIterator<Item = crate::Result<InternalValue>>> DoubleEndedIterator
    for MvccStream<I>
{
    fn next_back(&mut self) -> Option<Self::Item> {
        // Buffer for collecting entries in merge-aware reverse iteration
        let mut key_entries: Vec<InternalValue> = Vec::new();

        loop {
            let tail = fail_iter!(self.inner.next_back()?);

            let prev = match self.inner.peek_back() {
                Some(Ok(prev)) => prev,
                Some(Err(_)) => {
                    #[expect(
                        clippy::expect_used,
                        reason = "we just asserted, the peeked value is an error"
                    )]
                    return Some(Err(self
                        .inner
                        .next_back()
                        .expect("should exist")
                        .expect_err("should be error")));
                }
                None => {
                    // Last item in iterator — check if we have buffered merge entries
                    if !key_entries.is_empty() {
                        key_entries.push(tail);
                        return Some(self.resolve_merge_buffered(key_entries));
                    }
                    // Check if this single entry is a merge operand
                    if tail.key.value_type.is_merge_operand() {
                        if let Some(merge_op) = self.merge_operator.clone() {
                            return Some(self.resolve_merge_single(&tail, merge_op.as_ref()));
                        }
                    }
                    return Some(Ok(tail));
                }
            };

            if prev.key.user_key < tail.key.user_key {
                // `tail` is the newest entry for this key
                if !key_entries.is_empty() {
                    key_entries.push(tail);
                    return Some(self.resolve_merge_buffered(key_entries));
                }
                // Check if this single entry needs merge resolution
                if tail.key.value_type.is_merge_operand() {
                    if let Some(merge_op) = self.merge_operator.clone() {
                        return Some(self.resolve_merge_single(&tail, merge_op.as_ref()));
                    }
                }
                return Some(Ok(tail));
            }

            // Same key — if merge operator is configured and any entry is MergeOperand,
            // we need to buffer entries for merge resolution
            if self.merge_operator.is_some() && tail.key.value_type.is_merge_operand() {
                key_entries.push(tail);
            } else if !key_entries.is_empty() {
                // Already buffering for this key — continue
                key_entries.push(tail);
            }
            // Otherwise: normal behavior — just skip older versions (loop continues)
        }
    }
}

#[cfg(test)]
#[expect(clippy::string_lit_as_bytes)]
mod tests {
    use super::*;
    use crate::{value::InternalValue, ValueType};
    use test_log::test;

    macro_rules! stream {
      ($($key:expr, $sub_key:expr, $value_type:expr),* $(,)?) => {{
          let mut values = Vec::new();
          let mut counters = std::collections::HashMap::new();

          $(
              let key = $key.as_bytes();
              let sub_key = $sub_key.as_bytes();
              let value_type = match $value_type {
                  "V" => ValueType::Value,
                  "T" => ValueType::Tombstone,
                  "W" => ValueType::WeakTombstone,
                  _ => panic!("Unknown value type"),
              };

              let counter = counters.entry($key).and_modify(|x| { *x -= 1 }).or_insert(999);
              values.push(InternalValue::from_components(key, sub_key, *counter, value_type));
          )*

          values
      }};
    }

    macro_rules! iter_closed {
        ($iter:expr) => {
            assert!($iter.next().is_none(), "iterator should be closed (done)");
            assert!(
                $iter.next_back().is_none(),
                "iterator should be closed (done)"
            );
        };
    }

    /// Tests that the iterator emit the same stuff forwards and backwards, just in reverse
    macro_rules! test_reverse {
        ($v:expr) => {
            let iter = Box::new($v.iter().cloned().map(Ok));
            let iter = MvccStream::new(iter, None);
            let mut forwards = iter.flatten().collect::<Vec<_>>();
            forwards.reverse();

            let iter = Box::new($v.iter().cloned().map(Ok));
            let iter = MvccStream::new(iter, None);
            let backwards = iter.rev().flatten().collect::<Vec<_>>();

            assert_eq!(forwards, backwards);
        };
    }

    #[test]
    #[expect(clippy::unwrap_used)]
    fn mvcc_stream_error() -> crate::Result<()> {
        {
            let vec = [
                Ok(InternalValue::from_components(
                    "a",
                    "new",
                    999,
                    ValueType::Value,
                )),
                Err(crate::Error::Io(std::io::Error::other("test error"))),
            ];

            let iter = Box::new(vec.into_iter());
            let mut iter = MvccStream::new(iter, None);

            // Because next calls drain_key_min, the error is immediately first, even though
            // the first item is technically Ok
            assert!(matches!(iter.next().unwrap(), Err(crate::Error::Io(_))));
            iter_closed!(iter);
        }

        {
            let vec = [
                Ok(InternalValue::from_components(
                    "a",
                    "new",
                    999,
                    ValueType::Value,
                )),
                Err(crate::Error::Io(std::io::Error::other("test error"))),
            ];

            let iter = Box::new(vec.into_iter());
            let mut iter = MvccStream::new(iter, None);

            assert!(matches!(
                iter.next_back().unwrap(),
                Err(crate::Error::Io(_))
            ));
            assert_eq!(
                InternalValue::from_components(*b"a", *b"new", 999, ValueType::Value),
                iter.next_back().unwrap()?,
            );
            iter_closed!(iter);
        }

        Ok(())
    }

    #[test]
    #[expect(clippy::unwrap_used)]
    fn mvcc_queue_reverse_almost_gone() -> crate::Result<()> {
        let vec = [
            InternalValue::from_components("a", "a", 0, ValueType::Value),
            InternalValue::from_components("b", "", 1, ValueType::Tombstone),
            InternalValue::from_components("b", "b", 0, ValueType::Value),
            InternalValue::from_components("c", "", 1, ValueType::Tombstone),
            InternalValue::from_components("c", "c", 0, ValueType::Value),
            InternalValue::from_components("d", "", 1, ValueType::Tombstone),
            InternalValue::from_components("d", "d", 0, ValueType::Value),
            InternalValue::from_components("e", "", 1, ValueType::Tombstone),
            InternalValue::from_components("e", "e", 0, ValueType::Value),
        ];

        let iter = Box::new(vec.iter().cloned().map(Ok));

        let mut iter = MvccStream::new(iter, None);

        assert_eq!(
            InternalValue::from_components(*b"a", *b"a", 0, ValueType::Value),
            iter.next().unwrap()?,
        );
        assert_eq!(
            InternalValue::from_components(*b"b", *b"", 1, ValueType::Tombstone),
            iter.next().unwrap()?,
        );
        assert_eq!(
            InternalValue::from_components(*b"c", *b"", 1, ValueType::Tombstone),
            iter.next().unwrap()?,
        );
        assert_eq!(
            InternalValue::from_components(*b"d", *b"", 1, ValueType::Tombstone),
            iter.next().unwrap()?,
        );
        assert_eq!(
            InternalValue::from_components(*b"e", *b"", 1, ValueType::Tombstone),
            iter.next().unwrap()?,
        );
        iter_closed!(iter);

        test_reverse!(vec);

        Ok(())
    }

    #[test]
    #[expect(clippy::unwrap_used)]
    fn mvcc_queue_almost_gone_2() -> crate::Result<()> {
        let vec = [
            InternalValue::from_components("a", "a", 0, ValueType::Value),
            InternalValue::from_components("b", "", 1, ValueType::Tombstone),
            InternalValue::from_components("c", "", 1, ValueType::Tombstone),
            InternalValue::from_components("d", "", 1, ValueType::Tombstone),
            InternalValue::from_components("e", "", 1, ValueType::Tombstone),
        ];

        let iter = Box::new(vec.iter().cloned().map(Ok));

        let mut iter = MvccStream::new(iter, None);

        assert_eq!(
            InternalValue::from_components(*b"a", *b"a", 0, ValueType::Value),
            iter.next().unwrap()?,
        );
        assert_eq!(
            InternalValue::from_components(*b"b", *b"", 1, ValueType::Tombstone),
            iter.next().unwrap()?,
        );
        assert_eq!(
            InternalValue::from_components(*b"c", *b"", 1, ValueType::Tombstone),
            iter.next().unwrap()?,
        );
        assert_eq!(
            InternalValue::from_components(*b"d", *b"", 1, ValueType::Tombstone),
            iter.next().unwrap()?,
        );
        assert_eq!(
            InternalValue::from_components(*b"e", *b"", 1, ValueType::Tombstone),
            iter.next().unwrap()?,
        );
        iter_closed!(iter);

        test_reverse!(vec);

        Ok(())
    }

    #[test]
    #[expect(clippy::unwrap_used)]
    fn mvcc_queue() -> crate::Result<()> {
        let vec = [
            InternalValue::from_components("a", "a", 0, ValueType::Value),
            InternalValue::from_components("b", "b", 0, ValueType::Value),
            InternalValue::from_components("c", "c", 0, ValueType::Value),
            InternalValue::from_components("d", "d", 0, ValueType::Value),
            InternalValue::from_components("e", "", 1, ValueType::Tombstone),
            InternalValue::from_components("e", "e", 0, ValueType::Value),
        ];

        let iter = Box::new(vec.iter().cloned().map(Ok));

        let mut iter = MvccStream::new(iter, None);

        assert_eq!(
            InternalValue::from_components(*b"a", *b"a", 0, ValueType::Value),
            iter.next().unwrap()?,
        );
        assert_eq!(
            InternalValue::from_components(*b"b", *b"b", 0, ValueType::Value),
            iter.next().unwrap()?,
        );
        assert_eq!(
            InternalValue::from_components(*b"c", *b"c", 0, ValueType::Value),
            iter.next().unwrap()?,
        );
        assert_eq!(
            InternalValue::from_components(*b"d", *b"d", 0, ValueType::Value),
            iter.next().unwrap()?,
        );
        assert_eq!(
            InternalValue::from_components(*b"e", *b"", 1, ValueType::Tombstone),
            iter.next().unwrap()?,
        );
        iter_closed!(iter);

        test_reverse!(vec);

        Ok(())
    }

    #[test]
    #[expect(clippy::unwrap_used)]
    fn mvcc_queue_weak_almost_gone() -> crate::Result<()> {
        let vec = [
            InternalValue::from_components("a", "a", 0, ValueType::Value),
            InternalValue::from_components("b", "", 1, ValueType::WeakTombstone),
            InternalValue::from_components("b", "b", 0, ValueType::Value),
            InternalValue::from_components("c", "", 1, ValueType::WeakTombstone),
            InternalValue::from_components("c", "c", 0, ValueType::Value),
            InternalValue::from_components("d", "", 1, ValueType::WeakTombstone),
            InternalValue::from_components("d", "d", 0, ValueType::Value),
            InternalValue::from_components("e", "", 1, ValueType::WeakTombstone),
            InternalValue::from_components("e", "e", 0, ValueType::Value),
        ];

        let iter = Box::new(vec.iter().cloned().map(Ok));

        let mut iter = MvccStream::new(iter, None);

        assert_eq!(
            InternalValue::from_components(*b"a", *b"a", 0, ValueType::Value),
            iter.next().unwrap()?,
        );
        assert_eq!(
            InternalValue::from_components(*b"b", *b"", 1, ValueType::WeakTombstone),
            iter.next().unwrap()?,
        );
        assert_eq!(
            InternalValue::from_components(*b"c", *b"", 1, ValueType::WeakTombstone),
            iter.next().unwrap()?,
        );
        assert_eq!(
            InternalValue::from_components(*b"d", *b"", 1, ValueType::WeakTombstone),
            iter.next().unwrap()?,
        );
        assert_eq!(
            InternalValue::from_components(*b"e", *b"", 1, ValueType::WeakTombstone),
            iter.next().unwrap()?,
        );
        iter_closed!(iter);

        test_reverse!(vec);

        Ok(())
    }

    #[test]
    #[expect(clippy::unwrap_used)]
    fn mvcc_queue_weak_almost_gone_2() -> crate::Result<()> {
        let vec = [
            InternalValue::from_components("a", "a", 0, ValueType::Value),
            InternalValue::from_components("b", "", 1, ValueType::WeakTombstone),
            InternalValue::from_components("c", "", 1, ValueType::WeakTombstone),
            InternalValue::from_components("d", "", 1, ValueType::WeakTombstone),
            InternalValue::from_components("e", "", 1, ValueType::WeakTombstone),
        ];

        let iter = Box::new(vec.iter().cloned().map(Ok));

        let mut iter = MvccStream::new(iter, None);

        assert_eq!(
            InternalValue::from_components(*b"a", *b"a", 0, ValueType::Value),
            iter.next().unwrap()?,
        );
        assert_eq!(
            InternalValue::from_components(*b"b", *b"", 1, ValueType::WeakTombstone),
            iter.next().unwrap()?,
        );
        assert_eq!(
            InternalValue::from_components(*b"c", *b"", 1, ValueType::WeakTombstone),
            iter.next().unwrap()?,
        );
        assert_eq!(
            InternalValue::from_components(*b"d", *b"", 1, ValueType::WeakTombstone),
            iter.next().unwrap()?,
        );
        assert_eq!(
            InternalValue::from_components(*b"e", *b"", 1, ValueType::WeakTombstone),
            iter.next().unwrap()?,
        );
        iter_closed!(iter);

        test_reverse!(vec);

        Ok(())
    }

    #[test]
    #[expect(clippy::unwrap_used)]
    fn mvcc_queue_weak_reverse() -> crate::Result<()> {
        let vec = [
            InternalValue::from_components("a", "a", 0, ValueType::Value),
            InternalValue::from_components("b", "b", 0, ValueType::Value),
            InternalValue::from_components("c", "c", 0, ValueType::Value),
            InternalValue::from_components("d", "d", 0, ValueType::Value),
            InternalValue::from_components("e", "", 1, ValueType::WeakTombstone),
            InternalValue::from_components("e", "e", 0, ValueType::Value),
        ];

        let iter = Box::new(vec.iter().cloned().map(Ok));

        let mut iter = MvccStream::new(iter, None);

        assert_eq!(
            InternalValue::from_components(*b"a", *b"a", 0, ValueType::Value),
            iter.next().unwrap()?,
        );
        assert_eq!(
            InternalValue::from_components(*b"b", *b"b", 0, ValueType::Value),
            iter.next().unwrap()?,
        );
        assert_eq!(
            InternalValue::from_components(*b"c", *b"c", 0, ValueType::Value),
            iter.next().unwrap()?,
        );
        assert_eq!(
            InternalValue::from_components(*b"d", *b"d", 0, ValueType::Value),
            iter.next().unwrap()?,
        );
        assert_eq!(
            InternalValue::from_components(*b"e", *b"", 1, ValueType::WeakTombstone),
            iter.next().unwrap()?,
        );
        iter_closed!(iter);

        test_reverse!(vec);

        Ok(())
    }

    #[test]
    #[expect(clippy::unwrap_used)]
    fn mvcc_stream_simple() -> crate::Result<()> {
        #[rustfmt::skip]
        let vec = stream![
          "a", "new", "V",
          "a", "old", "V",
        ];

        let iter = Box::new(vec.iter().cloned().map(Ok));

        let mut iter = MvccStream::new(iter, None);

        assert_eq!(
            InternalValue::from_components(*b"a", *b"new", 999, ValueType::Value),
            iter.next().unwrap()?,
        );
        iter_closed!(iter);

        test_reverse!(vec);

        Ok(())
    }

    #[test]
    #[expect(clippy::unwrap_used)]
    fn mvcc_stream_simple_multi_keys() -> crate::Result<()> {
        #[rustfmt::skip]
        let vec = stream![
          "a", "new", "V",
          "a", "old", "V",
          "b", "new", "V",
          "b", "old", "V",
          "c", "newnew", "V",
          "c", "new", "V",
          "c", "old", "V",
        ];

        let iter = Box::new(vec.iter().cloned().map(Ok));

        let mut iter = MvccStream::new(iter, None);

        assert_eq!(
            InternalValue::from_components(*b"a", *b"new", 999, ValueType::Value),
            iter.next().unwrap()?,
        );
        assert_eq!(
            InternalValue::from_components(*b"b", *b"new", 999, ValueType::Value),
            iter.next().unwrap()?,
        );
        assert_eq!(
            InternalValue::from_components(*b"c", *b"newnew", 999, ValueType::Value),
            iter.next().unwrap()?,
        );
        iter_closed!(iter);

        test_reverse!(vec);

        Ok(())
    }

    #[test]
    #[expect(clippy::unwrap_used)]
    fn mvcc_stream_tombstone() -> crate::Result<()> {
        #[rustfmt::skip]
        let vec = stream![
          "a", "", "T",
          "a", "old", "V",
        ];

        let iter = Box::new(vec.iter().cloned().map(Ok));

        let mut iter = MvccStream::new(iter, None);

        assert_eq!(
            InternalValue::from_components(*b"a", *b"", 999, ValueType::Tombstone),
            iter.next().unwrap()?,
        );
        iter_closed!(iter);

        test_reverse!(vec);

        Ok(())
    }

    #[test]
    #[expect(clippy::unwrap_used)]
    fn mvcc_stream_tombstone_multi_keys() -> crate::Result<()> {
        #[rustfmt::skip]
        let vec = stream![
          "a", "", "T",
          "a", "old", "V",
          "b", "", "T",
          "b", "old", "V",
          "c", "", "T",
          "c", "", "T",
          "c", "old", "V",
        ];

        let iter = Box::new(vec.iter().cloned().map(Ok));

        let mut iter = MvccStream::new(iter, None);

        assert_eq!(
            InternalValue::from_components(*b"a", *b"", 999, ValueType::Tombstone),
            iter.next().unwrap()?,
        );
        assert_eq!(
            InternalValue::from_components(*b"b", *b"", 999, ValueType::Tombstone),
            iter.next().unwrap()?,
        );
        assert_eq!(
            InternalValue::from_components(*b"c", *b"", 999, ValueType::Tombstone),
            iter.next().unwrap()?,
        );
        iter_closed!(iter);

        test_reverse!(vec);

        Ok(())
    }

    #[test]
    #[expect(clippy::unwrap_used)]
    fn mvcc_stream_weak_tombstone_simple() -> crate::Result<()> {
        #[rustfmt::skip]
        let vec = stream![
          "a", "", "W",
          "a", "old", "V",
        ];

        let iter = Box::new(vec.iter().cloned().map(Ok));

        let mut iter = MvccStream::new(iter, None);

        assert_eq!(
            InternalValue::from_components(*b"a", *b"", 999, ValueType::WeakTombstone),
            iter.next().unwrap()?,
        );
        iter_closed!(iter);

        test_reverse!(vec);

        Ok(())
    }

    #[test]
    #[expect(clippy::unwrap_used)]
    fn mvcc_stream_weak_tombstone_resurrection() -> crate::Result<()> {
        #[rustfmt::skip]
        let vec = stream![
          "a", "", "W",
          "a", "new", "V",
          "a", "old", "V",
        ];

        let iter = Box::new(vec.iter().cloned().map(Ok));

        let mut iter = MvccStream::new(iter, None);

        assert_eq!(
            InternalValue::from_components(*b"a", *b"", 999, ValueType::WeakTombstone),
            iter.next().unwrap()?,
        );
        iter_closed!(iter);

        test_reverse!(vec);

        Ok(())
    }

    #[test]
    #[expect(clippy::unwrap_used)]
    fn mvcc_stream_weak_tombstone_priority() -> crate::Result<()> {
        #[rustfmt::skip]
        let vec = stream![
          "a", "", "T",  
          "a", "", "W",
          "a", "new", "V",
          "a", "old", "V",
        ];

        let iter = Box::new(vec.iter().cloned().map(Ok));

        let mut iter = MvccStream::new(iter, None);

        assert_eq!(
            InternalValue::from_components(*b"a", *b"", 999, ValueType::Tombstone),
            iter.next().unwrap()?,
        );
        iter_closed!(iter);

        test_reverse!(vec);

        Ok(())
    }

    #[test]
    #[expect(clippy::unwrap_used)]
    fn mvcc_stream_weak_tombstone_multi_keys() -> crate::Result<()> {
        #[rustfmt::skip]
        let vec = stream![
          "a", "", "W",
          "a", "old", "V",
          "b", "", "W",
          "b", "old", "V",
          "c", "", "W",
          "c", "old", "V",
        ];

        let iter = Box::new(vec.iter().cloned().map(Ok));

        let mut iter = MvccStream::new(iter, None);

        assert_eq!(
            InternalValue::from_components(*b"a", *b"", 999, ValueType::WeakTombstone),
            iter.next().unwrap()?,
        );
        assert_eq!(
            InternalValue::from_components(*b"b", *b"", 999, ValueType::WeakTombstone),
            iter.next().unwrap()?,
        );
        assert_eq!(
            InternalValue::from_components(*b"c", *b"", 999, ValueType::WeakTombstone),
            iter.next().unwrap()?,
        );
        iter_closed!(iter);

        test_reverse!(vec);

        Ok(())
    }
}
