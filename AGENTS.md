# Repository Guidance

## File Size

- Keep hand-authored source and documentation files under 300 lines of code where practical.
- Treat 500 lines of code as a hard ceiling for hand-authored files.
- Split files by cohesive ownership when they approach the soft limit. Prefer feature modules, sibling views, or linked docs over large catch-all files.
- Generated, vendored, lockfile, binary, and benchmark-output artifacts are exempt from the LOC ceiling, but keep their producers small and documented.
