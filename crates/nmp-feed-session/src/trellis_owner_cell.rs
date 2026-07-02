use std::cell::{Cell, UnsafeCell};
use std::thread::{self, ThreadId};

pub(super) struct ActorThreadCell<T> {
    owner: ThreadId,
    borrowed: Cell<bool>,
    value: UnsafeCell<T>,
}

// Safety: callers can move/share the handle to satisfy callback traits, but
// `with_ref`/`with_mut` only expose the value on the creating actor thread.
unsafe impl<T: Send> Send for ActorThreadCell<T> {}
unsafe impl<T: Send> Sync for ActorThreadCell<T> {}

impl<T> ActorThreadCell<T> {
    pub(super) fn new(value: T) -> Self {
        Self {
            owner: thread::current().id(),
            borrowed: Cell::new(false),
            value: UnsafeCell::new(value),
        }
    }

    pub(super) fn with_mut<R>(&self, operation: &'static str, f: impl FnOnce(&mut T) -> R) -> R {
        self.assert_owner(operation);
        self.with_exclusive_borrow(operation, || f(unsafe { &mut *self.value.get() }))
    }

    #[cfg(test)]
    pub(super) fn with_ref<R>(&self, operation: &'static str, f: impl FnOnce(&T) -> R) -> R {
        self.assert_owner(operation);
        self.with_exclusive_borrow(operation, || f(unsafe { &*self.value.get() }))
    }

    fn with_exclusive_borrow<R>(&self, operation: &'static str, f: impl FnOnce() -> R) -> R {
        assert!(
            !self.borrowed.replace(true),
            "FeedSessionTrellisAdapter {operation} re-entered while the Trellis graph is borrowed"
        );
        let _guard = ActorThreadBorrowGuard {
            borrowed: &self.borrowed,
        };
        f()
    }

    fn assert_owner(&self, operation: &'static str) {
        assert_eq!(
            thread::current().id(),
            self.owner,
            "FeedSessionTrellisAdapter {operation} called outside its owner actor thread"
        );
    }
}

struct ActorThreadBorrowGuard<'a> {
    borrowed: &'a Cell<bool>,
}

impl Drop for ActorThreadBorrowGuard<'_> {
    fn drop(&mut self) {
        self.borrowed.set(false);
    }
}
