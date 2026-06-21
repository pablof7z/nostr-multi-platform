---
title: FlatBuffers Decode Validation
slug: flatbuffers-decode-validation
topic: ffi-runtime
summary: The FlatBuffers decode_value function returns a Result and rejects invalid values (non-finite floats, missing string_value, missing list/map, missing map pair v
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-26
updated: 2026-05-26
verified: 2026-05-26
compiled-from: conversation
sources:
  - session:37e351ee-aa2b-43eb-9793-482de338f883
---

# FlatBuffers Decode Validation

## Value Decoding Validation

The FlatBuffers decode_value function returns a Result and rejects invalid values (non-finite floats, missing string_value, missing list/map, missing map pair values, unknown value kinds) as UpdateFrameDecodeError instead of degrading them to null. <!-- [^37e35-1] -->

## Degradation Tracking

A monotonic update_frame_degradations_total counter tracks update-frame encoding/decoding degradations observed at the Rust transport boundary, incrementing when serde_json::to_value fails and including the count in the degraded fallback snapshot payload. <!-- [^37e35-2] -->
