---
type: noun-entry
slug: nmp-nip60
name: "nmp-nip60"
origin: extracted
source_refs:
  - transcript:788-788
  - transcript:807-826
---

# nmp-nip60

NMP crate for NIP-60 Cashu wallet + NIP-61 NutZap + NIP-88 mint discovery event codecs, Cashu proof/DLEQ/P2PK/rollover types, and pure shape validation. NIP mechanics only — backend selection, the wallet operation journal, and the WalletBackend seam live in nmp-wallet. Performs zero relay I/O; the kernel fetches events and feeds them in via ingest_* methods.
