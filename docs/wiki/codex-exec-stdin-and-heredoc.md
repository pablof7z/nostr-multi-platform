---
title: "Codex Exec: Heredoc Input and Stdin Handling"
slug: codex-exec-stdin-and-heredoc
summary: Large prompts are passed to `codex exec` via bash heredoc substitution as a command-line argument, with stdin redirected from `/dev/null` to prevent the process
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

# Codex Exec: Heredoc Input and Stdin Handling

## Passing Large Prompts

When instructed to use codex, the `codex exec` command is used with a simple prompt directing codex to read a file, and the full prompt content is placed in that temporary file to prevent errors. The `< /dev/null` flag is always appended to codex exec commands to prevent stdin hang. (Previously: Large prompts were passed via bash heredoc substitution as a command-line argument.)

<!-- citations: [^96f16-1] [^f2605-3] -->

## Execution Duration

Codex exec runs typically take 20 minutes and must be allowed to finish without being killed. [^f2605-4]
## See Also

