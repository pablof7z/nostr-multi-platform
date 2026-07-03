use std::sync::Mutex;

pub(super) struct TrellisGraphCell<T> {
    value: Mutex<T>,
}

impl<T> TrellisGraphCell<T> {
    pub(super) fn new(value: T) -> Self {
        Self {
            value: Mutex::new(value),
        }
    }

    pub(super) fn with_mut<R>(&self, operation: &'static str, f: impl FnOnce(&mut T) -> R) -> R {
        let mut value = self.lock(operation);
        f(&mut value)
    }

    #[cfg(test)]
    pub(super) fn with_ref<R>(&self, operation: &'static str, f: impl FnOnce(&T) -> R) -> R {
        let value = self.lock(operation);
        f(&value)
    }

    fn lock(&self, operation: &'static str) -> std::sync::MutexGuard<'_, T> {
        self.value
            .lock()
            .unwrap_or_else(|_| panic!("FeedSessionTrellisAdapter {operation} lock poisoned"))
    }
}
