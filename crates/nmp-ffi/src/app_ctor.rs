//! Test-only constructor wrapper over `nmp-native-runtime`.

#[cfg(test)]
use crate::NmpApp;

#[cfg(test)]
pub(crate) fn test_app_new() -> *mut NmpApp {
    Box::into_raw(Box::new(nmp_native_runtime::new_app()))
}
