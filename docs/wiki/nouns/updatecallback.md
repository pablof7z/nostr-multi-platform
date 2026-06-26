---
type: noun-entry
slug: updatecallback
name: "UpdateCallback"
origin: extracted
source_refs:
  - transcript:198-203
---

# UpdateCallback

extern "C" function pointer type fn(*mut c_void, *const u8, usize) invoked when update frames arrive, with signature (context, frame_bytes_ptr, frame_byte_len)
