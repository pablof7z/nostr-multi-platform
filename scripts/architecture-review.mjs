#!/usr/bin/env node
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";

const DOC_PATHS = [
  "AGENTS.md",
  "docs/aim.md",
  "docs/builder-guide/03-doctrine-d0-d8.md",
  "docs/builder-guide/06-reactivity-contract.md",
  "docs/builder-guide/22-doctrine-checklist.md",
];

const DEFAULT_MAX_DIFF_CHARS = 140_000;
const DEFAULT_MAX_DOC_CHARS = 16_000;
function parseArgs(argv) {
  const args = {};
  for (let i = 0; i < argv.length; i += 1) {
    const token = argv[i];
    if (!token.startsWith("--")) {
      throw new Error(`unexpected positional argument: ${token}`);
    }
    const key = token.slice(2);
    const value = argv[i + 1];
    if (!value || value.startsWith("--")) {
      args[key] = "true";
    } else {
      args[key] = value;
      i += 1;
    }
  }
  return args;
}
function git(args) {
  return execFileSync("git", args, {
    encoding: "utf8",
    maxBuffer: 32 * 1024 * 1024,
  });
}
function readLimited(file, maxChars) {
  if (!fs.existsSync(file)) {
    return null;
  }
  const body = fs.readFileSync(file, "utf8");
  if (body.length <= maxChars) {
    return body;
  }
  return `${body.slice(0, maxChars)}\n\n[truncated at ${maxChars} chars]`;
}
function diffText(base, head, maxChars) {
  const diff = git([
    "diff",
    "--no-ext-diff",
    "--find-renames",
    "--unified=80",
    `${base}...${head}`,
  ]);
  if (diff.length <= maxChars) {
    return { diff, truncated: false };
  }
  return {
    diff: `${diff.slice(0, maxChars)}\n\n[diff truncated at ${maxChars} chars]`,
    truncated: true,
  };
}
function changedFiles(base, head) {
  return git(["diff", "--name-status", `${base}...${head}`]).trim();
}

function docsContext(maxChars) {
  return DOC_PATHS.map((docPath) => {
    const body = readLimited(docPath, maxChars);
    if (body == null) {
      return null;
    }
    return `--- ${docPath} ---\n${body}`;
  })
    .filter(Boolean)
    .join("\n\n");
}

function buildPrompt({ base, head, files, diff, docs, truncated }) {
  return `You are the required architecture merge gate for nostr-multi-platform.

Review whether this pull request introduces or worsens architectural violations.
Existing untouched debt is not a failing finding unless this PR relies on it,
extends it, weakens a guardrail, or makes it harder to remove.

Hard rules:
- Rust owns business logic. Native shells render Rust state and execute raw OS
  capabilities only. Native must not decide protocol policy, relay policy,
  routing, retries, caching, parsing, error recovery, or state transitions.
- No polling. No sleep/check loops, periodic timer queries, try_recv/sleep
  spins, or hidden timeout loops used as state progress. Use blocking
  primitives, callbacks, typed ticks, or explicit wall-clock events.
- NMP crates under crates/ contain reusable Nostr infrastructure only. App
  product policy belongs in apps/<app>/ or a dedicated app Rust crate.
- Effects are typed data crossing boundaries. Reducers remain replayable.
  Time, randomness, network callbacks, and capability completions must enter as
  explicit events/actions. Do not add direct publish/write doors that bypass the
  typed action boundary.
- Preserve provenance for relay/private-event paths and avoid logging secrets,
  raw nsecs, plaintext DMs, bearer tokens, or private payloads.
- Keep full snapshots as the correctness path; deltas must be lossless.

Return JSON only, with this shape:
{
  "verdict": "pass" | "fail",
  "summary": "one or two sentence assessment",
  "findings": [
    {
      "severity": "blocker" | "high" | "medium" | "low",
      "rule": "short rule name",
      "path": "repo-relative path",
      "line": 123,
      "evidence": "specific evidence from the diff",
      "required_fix": "what must change before merge"
    }
  ],
  "signoff": "short statement explaining why this is safe to merge, or why it is blocked"
}

Fail the review for blocker, high, or medium architectural findings. Low
findings may pass only when they are non-blocking cleanup notes.

Base SHA: ${base}
Head SHA: ${head}
Diff truncated: ${truncated ? "yes" : "no"}

Changed files:
${files || "(no changed files)"}

Architecture references:
${docs}

Pull request diff:
${diff}`;
}

function extractJson(text) {
  const trimmed = text.trim().replace(/^```(?:json)?/i, "").replace(/```$/i, "").trim();
  try {
    return JSON.parse(trimmed);
  } catch {
    const start = trimmed.indexOf("{");
    const end = trimmed.lastIndexOf("}");
    if (start >= 0 && end > start) {
      return JSON.parse(trimmed.slice(start, end + 1));
    }
    throw new Error("model response did not contain parseable JSON");
  }
}

async function callAnthropic({ model, prompt }) {
  const apiKey = process.env.ANTHROPIC_API_KEY;
  if (!apiKey) {
    throw new Error("ANTHROPIC_API_KEY is required for anthropic architecture review");
  }
  const response = await fetch("https://api.anthropic.com/v1/messages", {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      "x-api-key": apiKey,
      "anthropic-version": "2023-06-01",
    },
    body: JSON.stringify({
      model,
      max_tokens: 4096,
      temperature: 0,
      messages: [{ role: "user", content: prompt }],
    }),
  });
  const body = await response.json();
  if (!response.ok) {
    throw new Error(`anthropic review failed: ${JSON.stringify(body)}`);
  }
  return body.content?.map((part) => part.text || "").join("\n") || "";
}

async function callOpenAI({ model, prompt }) {
  const apiKey = process.env.OPENAI_API_KEY;
  if (!apiKey) {
    throw new Error("OPENAI_API_KEY is required for openai architecture review");
  }
  const response = await fetch("https://api.openai.com/v1/responses", {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      Authorization: `Bearer ${apiKey}`,
    },
    body: JSON.stringify({
      model,
      input: prompt,
    }),
  });
  const body = await response.json();
  if (!response.ok) {
    throw new Error(`openai review failed: ${JSON.stringify(body)}`);
  }
  if (body.output_text) {
    return body.output_text;
  }
  return (body.output || [])
    .flatMap((item) => item.content || [])
    .map((part) => part.text || "")
    .join("\n");
}

function mockReview(verdict) {
  return {
    verdict,
    summary: `mock architecture review returned ${verdict}`,
    findings: [],
    signoff: "mock provider used for local workflow validation only",
  };
}

function isEnabled(value) {
  return ["1", "true", "yes", "required"].includes(String(value || "").toLowerCase());
}

function skippedReview(reason) {
  return {
    verdict: "pass",
    summary: `architecture review skipped: ${reason}`,
    findings: [],
    signoff: "AI architecture signoff is not required until the provider, model, and secret are configured.",
  };
}

function normalizeReview(raw) {
  const verdict = raw.verdict === "pass" ? "pass" : "fail";
  const findings = Array.isArray(raw.findings) ? raw.findings : [];
  return {
    verdict,
    summary: String(raw.summary || ""),
    findings,
    signoff: String(raw.signoff || ""),
  };
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const base = args.base || process.env.GITHUB_BASE_SHA;
  const head = args.head || process.env.GITHUB_HEAD_SHA || "HEAD";
  if (!base) {
    throw new Error("--base or GITHUB_BASE_SHA is required");
  }

  const providerFromCli = Boolean(args.provider);
  const provider = args.provider || process.env.ARCHITECTURE_REVIEW_PROVIDER || "";
  const model = args.model || process.env.ARCHITECTURE_REVIEW_MODEL || "";
  const required = isEnabled(process.env.ARCHITECTURE_REVIEW_REQUIRED);
  const outPath = args.out || "architecture-review.json";
  if (!provider) {
    if (!required) {
      fs.writeFileSync(
        outPath,
        `${JSON.stringify(skippedReview("provider is not configured"), null, 2)}\n`
      );
      return;
    }
    throw new Error(
      "ARCHITECTURE_REVIEW_PROVIDER or --provider is required; use anthropic/openai in CI"
    );
  }
  if (provider === "mock" && !providerFromCli) {
    throw new Error("mock architecture review is local-only; pass --provider mock explicitly");
  }
  if (provider === "mock" && process.env.GITHUB_ACTIONS === "true") {
    throw new Error("mock architecture review is not allowed in GitHub Actions");
  }
  if (provider !== "mock" && !model) {
    if (!required) {
      fs.writeFileSync(
        outPath,
        `${JSON.stringify(skippedReview("model is not configured"), null, 2)}\n`
      );
      return;
    }
    throw new Error("ARCHITECTURE_REVIEW_MODEL or --model is required");
  }

  const maxDiffChars = Number(args["max-diff-chars"] || DEFAULT_MAX_DIFF_CHARS);
  const files = changedFiles(base, head);
  const { diff, truncated } = diffText(base, head, maxDiffChars);
  const prompt = buildPrompt({
    base,
    head,
    files,
    diff,
    docs: docsContext(DEFAULT_MAX_DOC_CHARS),
    truncated,
  });

  let review;
  if (provider === "mock") {
    review = mockReview(args["mock-verdict"] || "pass");
  } else if (provider === "anthropic") {
    review = extractJson(await callAnthropic({ model, prompt }));
  } else if (provider === "openai") {
    review = extractJson(await callOpenAI({ model, prompt }));
  } else {
    throw new Error(`unsupported architecture review provider: ${provider}`);
  }

  const normalized = normalizeReview(review);
  const report = {
    provider,
    model: provider === "mock" ? "mock" : model,
    required: true,
    base_sha: base,
    head_sha: head,
    generated_at: new Date().toISOString(),
    diff_truncated: truncated,
    changed_files: files.split("\n").filter(Boolean),
    ...normalized,
  };
  fs.mkdirSync(path.dirname(outPath), { recursive: true });
  fs.writeFileSync(outPath, `${JSON.stringify(report, null, 2)}\n`);

  console.log(`# Architecture Review\n\nVerdict: ${report.verdict}\n\n${report.summary}\n`);
  for (const finding of report.findings) {
    console.log(`- ${finding.severity || "unknown"} ${finding.path || ""}:${finding.line || ""} ${finding.rule || ""} - ${finding.evidence || ""}`);
  }
  if (report.verdict !== "pass") {
    process.exitCode = 1;
  }
}

main().catch((error) => {
  console.error(`architecture-review: ${error.message}`);
  process.exit(2);
});
