---
title: Passing large prompts to codex exec via heredoc with stdin handling
slug: codex-exec-stdin-heredoc
summary: Large prompts for `codex exec` are passed using bash heredoc substitution with stdin closed via `/dev/null`
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-17
updated: 2026-05-26
verified: 2026-05-17
compiled-from: conversation
sources:
  - session:96f16791-4f46-41cd-8afb-3d041e9fa7a5
  - session:f26050da-6d8a-4128-9179-4088a9df94b9
---

# Passing large prompts to codex exec via heredoc with stdin handling

## Passing Large Prompts via Heredoc

The `codex exec` command is invoked with a simple prompt directing codex to read a temporary file containing the full instructions, and `< /dev/null` is always appended. Codex exec takes approximately 20 minutes to finish and must not be killed early.

<!-- citations: [^96f16-1] [^f2605-5] -->
## See Also

