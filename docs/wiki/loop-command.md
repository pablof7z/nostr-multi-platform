---
title: /loop — Recurring and Self-Paced Prompt Scheduling
slug: loop-command
summary: The /loop command schedules a recurring or self-paced prompt, supporting fixed-interval cron scheduling and dynamic event-gated self-pacing with monitor-based wake signals.
tags:
  - loop
  - scheduling
  - cron
  - dynamic
  - self-pacing
  - monitor
  - skill
volatility: warm
confidence: medium
created: 2026-05-29
updated: 2026-05-29
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:ae88711c-a987-4b41-939e-32c8ee0ab4d3
---

# /loop — Recurring and Self-Paced Prompt Scheduling

> The /loop command schedules a recurring or self-paced prompt, supporting fixed-interval cron scheduling and dynamic event-gated self-pacing with monitor-based wake signals.

## Overview

The /loop command accepts an optional interval and a prompt, then schedules recurring execution. It supports two modes: fixed-interval (using cron) and dynamic self-paced (using monitors and scheduled wakeups). The command always runs the prompt immediately on first invocation and then schedules subsequent iterations. [^ae887-1]

## Parsing

Parse the input into [interval] <prompt…> using three rules in priority order.

Rule 1 — Leading token: If the first whitespace-delimited token matches ^\d+[smhd]$ (e.g. 5m, 2h), that's the interval; the rest is the prompt.

Rule 2 — Trailing "every" clause: Otherwise, if the input ends with "every <N><unit>" or "every <N> <unit-word>" (e.g. "every 20m", "every 5 minutes", "every 2 hours"), extract that as the interval and strip it from the prompt. Only match when what follows "every" is a time expression — "check every PR" has no interval.

Rule 3 — No interval: Otherwise, the entire input is the prompt and you'll self-pace dynamically.

If the resulting prompt is empty, show usage /loop [interval] <prompt> and stop. [^ae887-2]

## Parsing Examples

5m /babysit-prs → interval 5m, prompt /babysit-prs (rule 1). check the deploy every 20m → interval 20m, prompt check the deploy (rule 2). run tests every 5 minutes → interval 5m, prompt run tests (rule 2). check the deploy → no interval → dynamic mode, prompt check the deploy (rule 3). check every PR → no interval → dynamic mode, prompt check every PR (rule 3 — "every" not followed by time). 5m → empty prompt → show usage. [^ae887-3]

## Cloud Offer

Before any scheduling step, check whether EITHER is true: (a) the parsed interval (rule 1 or 2) is ≥60 minutes, or (b) regardless of which rule matched, the original input uses daily phrasing ("every morning", "daily", "every day", "each night", "every weekday").

If either is true, call AskUserQuestion first with question "This loop stops when you close this session. Set it up as a cloud schedule instead so it keeps running?", header "Schedule", and options [{label: "Cloud schedule (recommended)", description: "Runs in Anthropic's cloud even after you close this session"}, {label: "This session only", description: "Runs in this terminal until you exit"}].

If they pick Cloud schedule: do NOT call CronCreate. Invoke the schedule skill directly via the Skill tool with args set to their original input verbatim, then follow that skill's instructions to completion. Do NOT tell the user to run /schedule themselves. Then stop — do not continue to any section below (no CronCreate, no ScheduleWakeup, no "execute the prompt now").

If they pick This session only and the trigger was a parsed ≥60-minute interval: continue below with that interval. If the trigger was daily phrasing only (rule 3, no parsed interval): do NOT call CronCreate; explain that a daily-cadence loop won't fire before this session closes, suggest they either pick Cloud schedule or re-run /loop with an explicit shorter interval, then stop.

If neither trigger condition was met: continue below. [^ae887-4]

## Fixed-Interval Mode — Cron Conversion

Convert the interval to a cron expression using the following mapping:

| Nm where N ≤ 59: */N * * * * (every N minutes)
| Nm where N ≥ 60: 0 */H * * * * where H = N/60, must divide 24
| Nh where N ≤ 23: 0 */N * * * (every N hours)
| Nd: 0 0 */N * * (every N days at midnight local)
| Ns: treat as ceil(N/60)m (cron minimum granularity is 1 minute)

If the interval doesn't cleanly divide its unit (e.g. 7m → */7 * * * * gives uneven gaps at :56→:00; 90m → 1.5h which cron can't express), pick the nearest clean interval and tell the user what you rounded to before scheduling. [^ae887-5]

## Dynamic Mode — Overview

When no interval is parsed (rule 3), the user wants self-pacing. Decide what makes the next iteration worth running — a passage of time, or an observable event. Run the parsed prompt now. If the next run is gated on an event (CI finishing, a log line matching, a file changing, a PR comment) and no Monitor is already running for it: arm one now with persistent: true. Its events arrive as <task-notification> messages and wake this loop immediately — do not wait for the ScheduleWakeup deadline. Arm once; on later iterations call TaskList first and skip this step if a monitor is already running. [^ae887-7]

## Dynamic Mode — Confirmation and Scheduling

Briefly confirm: that you're self-pacing, whether a Monitor is the primary wake signal, that you ran the task now, and what fallback delay you're about to pick. Write this as text before calling ScheduleWakeup — the turn ends as soon as that tool returns.

Then, as the last action of this turn, call ScheduleWakeup with delaySeconds: with a Monitor armed this is the fallback heartbeat — how long to wait if no event fires (lean 1200–1800s; idle ticks past the 5-minute cache window are pure overhead). Without a Monitor this is the cadence — pick based on what you observed. reason: one short sentence on why you picked that delay. prompt: the full original /loop input verbatim, prefixed with /loop  so the next firing re-enters this skill and continues the loop. For example, if the user typed /loop check the deploy, pass /loop check the deploy as the prompt. [^ae887-8]

## Dynamic Mode — Wakeup Handling

If you were woken by a <task-notification> rather than the scheduled prompt: handle the event in the context of the loop task, then call ScheduleWakeup again with the same prompt and the same 1200–1800s delaySeconds — the Monitor remains the wake signal; this only resets the safety net. [^ae887-9]


When a task-notification fires for a rebase agent completing (e.g. "PR #790 rebase done — zero conflicts, CI re-running"), the loop handles the event in context: it notes the agent's result, updates the PR status, and continues waiting for remaining agents or CI. The ScheduleWakeup fallback timer is reset to the same delay range — the Monitor remains the primary wake signal for other in-flight agents. [^ae887-41]
## Dynamic Mode — Stopping the Loop

To stop the loop, omit the ScheduleWakeup call and TaskStop any Monitor you armed (use TaskList to find the task ID if it is no longer in context). Before you stop, send a one-line outcome via PushNotification — the user may be away and waiting to hear it's done. Skip this if you're stopping because the user just told you to; they're already here. [^ae887-10]

## Recurring Task Expiry

Recurring cron tasks auto-expire after 7 days. Users can cancel sooner with CronDelete, providing the job ID. [^ae887-11]

## Slash Command Dispatch

When the parsed prompt is a slash command (e.g. /babysit-prs), invoke it via the Skill tool. Otherwise act on the prompt directly. [^ae887-12]


## Fixed-Interval Mode — Execution

After determining the cron expression: (1) Call CronCreate with: cron (the expression), prompt (the parsed prompt verbatim), recurring: true. (2) Briefly confirm: what's scheduled, the cron expression, the human-readable cadence, that recurring tasks auto-expire after 7 days, and that the user can cancel sooner with CronDelete (include the job ID). Only if you did NOT show the cloud-offer AskUserQuestion above (i.e., neither trigger condition applied), end the confirmation with this exact line on its own, italicized: "Runs until you close this session · For durable cloud-based loops, use /schedule". If the user already answered that question, omit this line. (3) Then immediately execute the parsed prompt now — don't wait for the first cron fire. If it's a slash command, invoke it via the Skill tool; otherwise act on it directly.

On each cron fire, the system re-presents the full /loop specification with the original input appended as an "## Input" section. The assistant recognizes this as a cron iteration (not a fresh user invocation) because the message is the full skill specification text rather than a direct user command. The assistant executes the prompt directly without re-creating the cron job, and may choose shorter sub-interval check-ins (e.g. 3–8 minute ScheduleWakeup calls) while waiting for specific events like CI completion — effectively layering dynamic self-pacing on top of the fixed cron cadence.

The confirmation message for fixed-interval loops includes: the cadence description, the cron expression, the job ID, the 7-day auto-expiry notice, and the CronDelete cancellation instruction. When the cloud offer was not shown (interval <60min and no daily phrasing), the confirmation ends with the session-scope disclaimer.

During a cron iteration, if the task is waiting on an observable event (CI completion, rebase agent finishing, cargo test going green), the assistant uses ScheduleWakeup with short delays (180–480 seconds) rather than waiting for the next cron fire. This sub-interval pacing allows the loop to respond promptly to events while the fixed cron cadence provides the outer safety net. Each sub-interval wakeup re-enters the loop and re-assesses state, continuing the dynamic pacing until the task completes or the outer cron interval fires. When the queue is empty with no pending work, the loop checks back on the full cron interval to detect new PRs or changes.

The user may also trigger the loop task manually by sending the prompt text directly (without /loop prefix). When the assistant is already in a cron loop context and the user sends the prompt verbatim, this is treated as an immediate re-check — the assistant executes the prompt directly, re-assesses state, and may issue a new ScheduleWakeup. This is distinct from a fresh /loop invocation and does not create a second cron job.

<!-- citations: [^ae887-29] [^ae887-30] [^ae887-31] [^ae887-32] [^ae887-33] [^ae887-40] [^ae887-44] [^ae887-49] -->

Fixed-Interval Mode — Execution

When a cron fires, the system presents the full /loop specification text with an "## Input" section containing the original user input verbatim. The assistant distinguishes cron iterations from fresh user invocations because the message is the full skill specification rather than a direct user command. The assistant executes the prompt directly without re-creating the cron job. The confirmation message (scheduled cadence, cron expression, job ID, 7-day expiry, CronDelete instruction) is only shown on the initial /loop invocation, not on subsequent cron fires. [^ae887-53]

The user may manually trigger the loop task by sending the prompt text directly without the /loop prefix. When the assistant is already in a cron loop context and the user sends the prompt verbatim, this is treated as an immediate re-check — the assistant executes the prompt directly, re-assesses state, and may issue a new ScheduleWakeup. This is distinct from a fresh /loop invocation and does not create a second cron job. This pattern was exercised repeatedly: the user sent "review open PRs and land them in master -- sync origin/master and master and ensure no work is ever lost" without /loop prefix to trigger immediate re-checks of PR status. [^ae887-54]

During cron iterations, when the task is blocked waiting for an observable event (CI completion, rebase agent finishing, cargo test running), the loop uses the waiting time productively rather than sitting idle: inspect the blocking PR's actual diff to understand what's changing, check for orphaned worktree-agent branches that could be pruned, verify CI status via multiple methods, and look for parallel work that can be done. Each ScheduleWakeup re-entry re-assesses the full state and continues productive waiting until the event resolves or the outer cron interval fires. [^ae887-61]

When a cron fires, the system presents the full /loop specification text with an "## Input" section containing the original user input verbatim. The assistant distinguishes cron iterations from fresh user invocations because the message is the full skill specification rather than a direct user command. The assistant executes the prompt directly without re-creating the cron job. The confirmation message (scheduled cadence, cron expression, job ID, 7-day expiry, CronDelete instruction) is only shown on the initial /loop invocation, not on subsequent cron fires. [^ae887-75]

The user may manually trigger the loop task by sending the prompt text directly without the /loop prefix. When the assistant is already in a cron loop context and the user sends the prompt verbatim, this is treated as an immediate re-check — the assistant executes the prompt directly, re-assesses state, and may issue a new ScheduleWakeup. This is distinct from a fresh /loop invocation and does not create a second cron job. This pattern was exercised repeatedly: the user sent "review open PRs and land them in master -- sync origin/master and master and ensure no work is ever lost" without /loop prefix to trigger immediate re-checks of PR status. [^ae887-76]

During cron iterations, when the task is blocked waiting for an observable event (CI completion, rebase agent finishing, cargo test running), the loop uses the waiting time productively rather than sitting idle: inspect the blocking PR's actual diff to understand what's changing, check for orphaned worktree-agent branches that could be pruned, verify CI status via multiple methods, and look for parallel work that can be done. Each ScheduleWakeup re-entry re-assesses the full state and continues productive waiting until the event resolves or the outer cron interval fires. [^ae887-77]

Both cron iterations and manual re-checks use the same execution path: the assistant executes the parsed prompt directly, re-assesses state, and issues a new ScheduleWakeup if work remains. The distinguishing factor is how the message arrives: a cron fire presents the full /loop specification text with an "## Input" section, while a manual re-check is the user sending the prompt verbatim. In both cases, the assistant does NOT re-create the cron job. The response format is contextual: cron iterations that find the queue empty produce a concise status block; manual re-checks produce a direct status response; both may issue ScheduleWakeup for continued monitoring. [^ae887-82]

On each cron fire, the assistant precedes its response with "Cron iteration —" followed by a brief status note (e.g., "Cron iteration — checking PR status", "Cron iteration — checking rebase agent status on PR #795"). This signals to the user that this is an automated iteration rather than a fresh command. [^ae887-94]

During cron iterations, when the task is blocked waiting for an observable event, the loop uses ScheduleWakeup with sub-interval delays rather than waiting for the next full cron fire. The delay is calibrated to the expected remaining time for the blocking event: 3-4 minutes (180-240s) when cargo test is already running and nearing completion (~7-10 min total runtime), 5 minutes (300s) when rebase agents are still pushing and CI hasn't yet re-triggered, 8 minutes (480s) when CI has just been re-triggered on fresh commits and the full suite must run, and the full cron interval (e.g. 15 minutes / 900s) when the queue is empty with no pending work. Each sub-interval wakeup re-enters the loop and re-assesses state, continuing the dynamic pacing until the task completes or the outer cron interval fires. [^ae887-101]
## See Also
- [[disk-pressure-kills-agent-fleet|Accumulated Worktrees Cause Disk Exhaustion — Prune After Every Merge]] — related guide
- [[never-merge-on-pending-cargo-test|Never Merge on Pending cargo test — Cross-Crate Suite Is Mandatory]] — related guide
- [[agent-push-to-master-violation|Sub-Agents in Worktrees Must Push to Branch and Open a PR — Never Push to Master]] — related guide
- [[pr-review-land-loop-workflow|PR Review-and-Land Loop — Automated Merge Workflow]] — related guide
- [[pr-review-land-loop-workflow|PR Review-and-Land Loop — Automated Merge Workflow]] — related guide
- [[pr-review-land-loop-workflow|PR Review-and-Land Loop — Automated Merge Workflow]] — related guide
- [[pr-review-land-loop-workflow|PR Review-and-Land Loop — Automated Merge Workflow]] — related guide

