---
type: noun-entry
slug: paymentport
name: "PaymentPort"
origin: extracted
source_refs:
  - transcript:196-196
  - transcript:212-213
  - transcript:312-312
---

# PaymentPort

The seam through which NIP-57 zaps emit a 'pay this' intent (PaymentIntent); the selected wallet backend fulfills it. Currently nmp-nip47 owns the PaymentPort implementation (WalletPaymentPort) injected into the zap chain at composition time.
