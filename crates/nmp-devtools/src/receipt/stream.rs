use std::collections::VecDeque;
use std::num::NonZeroUsize;

use super::XrayReceipt;

/// Bounded ordered receipt stream.
#[derive(Clone, Debug)]
pub struct XrayReceiptStream {
    capacity: usize,
    next_sequence: u64,
    receipts: VecDeque<XrayReceipt>,
}

impl XrayReceiptStream {
    #[must_use]
    pub fn new(capacity: NonZeroUsize) -> Self {
        Self {
            capacity: capacity.get(),
            next_sequence: 1,
            receipts: VecDeque::with_capacity(capacity.get()),
        }
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.receipts.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.receipts.is_empty()
    }

    #[must_use]
    pub fn next_sequence(&self) -> u64 {
        self.next_sequence
    }

    pub fn push(&mut self, mut receipt: XrayReceipt) -> u64 {
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.saturating_add(1);
        receipt.sequence = sequence;
        self.receipts.push_back(receipt);
        while self.receipts.len() > self.capacity {
            self.receipts.pop_front();
        }
        sequence
    }

    pub fn push_batch<I>(&mut self, receipts: I)
    where
        I: IntoIterator<Item = XrayReceipt>,
    {
        for receipt in receipts {
            self.push(receipt);
        }
    }

    pub fn receipts(&self) -> impl ExactSizeIterator<Item = &XrayReceipt> {
        self.receipts.iter()
    }

    #[must_use]
    pub fn snapshot(&self) -> Vec<XrayReceipt> {
        self.receipts.iter().cloned().collect()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct XrayRecordingConfig {
    pub max_receipts: NonZeroUsize,
}

impl XrayRecordingConfig {
    #[must_use]
    pub const fn new(max_receipts: NonZeroUsize) -> Self {
        Self { max_receipts }
    }
}

/// Runtime opt-in recorder. Disabled mode never invokes the receipt builder.
#[derive(Clone, Debug, Default)]
pub struct XrayReceiptRecorder {
    stream: Option<XrayReceiptStream>,
}

impl XrayReceiptRecorder {
    #[must_use]
    pub const fn disabled() -> Self {
        Self { stream: None }
    }

    #[must_use]
    pub fn enabled(config: XrayRecordingConfig) -> Self {
        Self {
            stream: Some(XrayReceiptStream::new(config.max_receipts)),
        }
    }

    #[must_use]
    pub fn is_enabled(&self) -> bool {
        self.stream.is_some()
    }

    pub fn record_with<F>(&mut self, build: F)
    where
        F: FnOnce() -> Vec<XrayReceipt>,
    {
        let Some(stream) = &mut self.stream else {
            return;
        };
        stream.push_batch(build());
    }

    #[must_use]
    pub fn snapshot(&self) -> Vec<XrayReceipt> {
        self.stream
            .as_ref()
            .map(XrayReceiptStream::snapshot)
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;
    use crate::{
        XrayProjectionContext, XrayReason, XrayReasonCode, XrayReceiptEventKind, XrayTimestamp,
        XrayTransactionMarker,
    };

    fn receipt(id: &str) -> XrayReceipt {
        XrayReceipt::new(
            XrayProjectionContext::new(
                "app.feed.home",
                "home",
                "owner",
                XrayReason::new(XrayReasonCode::FeedSessionSync),
            ),
            XrayTransactionMarker::new(7, 3),
            XrayTimestamp::new(42),
            XrayReceiptEventKind::Open,
            id,
            None,
        )
    }

    #[test]
    fn stream_assigns_ordered_sequences_and_retains_bounded_tail() {
        let mut stream = XrayReceiptStream::new(NonZeroUsize::new(2).unwrap());

        assert_eq!(stream.push(receipt("a")), 1);
        assert_eq!(stream.push(receipt("b")), 2);
        assert_eq!(stream.push(receipt("c")), 3);

        let snapshot = stream.snapshot();
        assert_eq!(snapshot.len(), 2);
        assert_eq!(snapshot[0].sequence, 2);
        assert_eq!(snapshot[0].resource_id, "b");
        assert_eq!(snapshot[1].sequence, 3);
        assert_eq!(snapshot[1].resource_id, "c");
        assert_eq!(stream.next_sequence(), 4);
    }

    #[test]
    fn disabled_recorder_does_not_invoke_builder() {
        let built = Cell::new(false);
        let mut recorder = XrayReceiptRecorder::disabled();

        recorder.record_with(|| {
            built.set(true);
            vec![receipt("never-built")]
        });

        assert!(!built.get());
        assert!(recorder.snapshot().is_empty());
    }
}
