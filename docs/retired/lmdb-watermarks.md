# Retired LMDB Watermarks

The persisted `WatermarkRow` / `nmp-watermarks` design was removed. Current
coverage state is the K3 coverage ledger (`nmp-coverage`) described by
legacy decision 0056 and the store coverage APIs.

The persisted claim-register design (`nmp-claims`, `nmp-claims-budget`,
`ClaimerId`, `StoreError::OverPinned`) was also removed. Current GC receives a
kernel-derived explicit pin set.
