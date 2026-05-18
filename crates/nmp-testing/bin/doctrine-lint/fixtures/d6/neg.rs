//! Negative D6 fixture — must produce zero findings.
//!
//! Demonstrates every exemption:
//!   - test code under #[cfg(test)] mod
//!   - Mutex / RwLock lock-poisoning unwrap (same line + multi-line chain)
//!   - explicit `// doctrine-allow: D6` annotation
//!   - SAFETY-commented unsafe block
//!   - assert! / debug_assert! (never flagged)

use std::sync::{Mutex, RwLock};

pub struct Store {
    a: Mutex<Vec<u8>>,
    b: RwLock<Vec<u8>>,
}

impl Store {
    pub fn push(&self, x: u8) {
        // Mutex lock-poisoning idiom: same-line.
        self.a.lock().unwrap().push(x);
    }

    pub fn push_multiline(&self, x: u8) {
        // Mutex lock-poisoning idiom: multi-line method chain.
        self.a
            .lock()
            .unwrap()
            .push(x);
    }

    pub fn read(&self) -> usize {
        // RwLock read-poisoning idiom.
        self.b.read().unwrap().len()
    }

    pub fn invariant_check(&self, must_be_set: Option<u8>) -> u8 {
        // Explicit allow with rationale.
        must_be_set.unwrap() // doctrine-allow: D6 — caller-supplied invariant: see contract
    }

    pub fn debug_assertion(&self) {
        assert!(self.a.lock().unwrap().len() < 1000);
        debug_assert_eq!(self.a.lock().unwrap().len(), 0);
    }
}

pub unsafe fn read_raw(p: *const u8) -> u8 {
    *p.as_ref().unwrap() // SAFETY: caller proves p is non-null + aligned
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_things() {
        let v: Option<u8> = Some(1);
        let _ = v.unwrap();
        let _ = v.expect("ok");
        panic!("test panic");
    }
}
