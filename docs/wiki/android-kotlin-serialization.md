---
title: Android Kotlin Serialization Conventions
slug: android-kotlin-serialization
summary: All serde field mappings use @SerialName("snake_case") on fields and camelCase on variant discriminators
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-18
updated: 2026-05-25
verified: 2026-05-18
compiled-from: conversation
sources:
  - session:29d2c220-a86b-4b0d-82fb-d40d8fd4505e
  - session:45258890-9aa6-4063-8df0-bdf7021e9f72
---

# Android Kotlin Serialization Conventions

## Serialization Conventions

All serde field mappings use @SerialName("snake_case") on fields and camelCase on variant discriminators. All sealed classes use JsonContentPolymorphicSerializer with unknown types falling back gracefully to an .Unknown(type) variant. The Kotlin WireNode sealed class uses @JsonClassDiscriminator("kind") to match Rust's #[serde(tag = "kind", rename_all = "snake_case")], and WireInvoice is a flat data class with three optional fields (bolt11, bolt12, cashu) to match Rust's externally-tagged JSON format.

<!-- citations: [^29d2c-1] [^45258-1] -->
## See Also

