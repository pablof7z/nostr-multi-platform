---
title: Agent Collision Handling — Two Agents Targeting One Branch
slug: collision-handling-two-agents-one-branch
summary: When two agents target the same branch, kill the duplicate before it can clobber the original's work. Check branch HEAD before dispatching fix agents.
tags:
  - multi-agent
  - workflow
  - collision
volatility: warm
confidence: medium
created: 2026-05-30
updated: 2026-05-26
verified: 2026-05-30
compiled-from: conversation
sources:
  - session:4edd41f1-8318-4a4b-98d8-de01ae35f81b
  - session:47203d35-d7c9-4c12-bc47-a40773d7acc2
  - session:56d215c4-1aee-47cc-95c2-fd17269b92b6
---

# Agent Collision Handling — Two Agents Targeting One Branch

> When two agents target the same branch, kill the duplicate before it can clobber the original's work. Check branch HEAD before dispatching fix agents.

## The Collision — Two Agents Targeting One Branch

In the V-104 e2e test item, a collision occurred: the original implementation agent (a20761fa9b628835f) independently replaced the fake negentropy test with the real T129 WatermarkFn-to-since mechanism and pushed it. Meanwhile, a second fix agent (a70ee62e07dac8197) was dispatched after the reviewer flagged the fake version — targeting the same branch. Two agents targeting one branch creates a clobber risk: the second agent could overwrite the first agent's already-pushed real fix with its own work. [^4edd4-113]

## Resolution — Kill the Duplicate

The correct resolution when two agents target the same branch is to kill the duplicate (the one that hasn't pushed yet) before it can clobber the original's work. In this case, the original agent had already pushed ca593f10 with the real T129 fix, and the fix agent hadn't pushed yet. Killing the duplicate prevented the clobber. The branch HEAD remained at the original agent's commit. A fresh review was then dispatched on the real fix rather than the fake version. [^4edd4-114]

## Root Cause — Review Timing

The collision happened because a late notification from the original agent arrived after the reviewer had already seen the fake version and a fix agent had been dispatched. The sequence: original agent pushed real fix → late notification arrived → but reviewer saw fake version first → dispatched fix agent → collision. The original agent had already fixed the issue before the reviewer even flagged it. [^4edd4-115]

## Avoidance

Before dispatching a fix agent, check whether the branch HEAD has already been updated by the original agent. If the branch tip has changed since the reviewer's verdict, re-review the current tip rather than dispatching a fix agent. Also, check for late notifications from the original agent that may indicate they completed work after the reviewer started but before the fix agent was dispatched. [^4edd4-116]


The agent concurrency cap is 10. [^47203-4]

Git push --force must never be used; after a squash merge, delete the remote branch then push fresh. [^56d21-1]
## See Also
- [[sonnet-review-gate-workflow|Sonnet Review Gate — Mandatory PR Review Before Merge]] — related guide
- [[multi-agent-integration-workflow|Multi-Agent Integration Workflow — Fan-Out with Integration Branch]] — related guide

