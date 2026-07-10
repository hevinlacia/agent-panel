/**
 * Tests for src/codeReview.ts.
 *
 * Covers the pure pieces of the code-review feature: risk-tag detection and
 * managed review.md block replacement. Git scanning itself is integration
 * behavior exercised through the dashboard route against real repos.
 */

import { mkdtempSync, mkdirSync } from "node:fs"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { test } from "node:test"
import { strict as assert } from "node:assert"

import {
  CODE_REVIEW_BLOCK_END,
  CODE_REVIEW_BLOCK_START,
  classifyCodeReviewRiskTags,
  parseUnifiedDiff,
  resolveCodeReviewProjectPath,
  upsertCodeReviewBlock,
  type CodeReviewSnapshot,
} from "../src/codeReview.ts"

function sampleSnapshot(summary = "人工确认通过"): CodeReviewSnapshot {
  return {
    version: 1,
    reqId: "WMS-016-shop-query-unify",
    updatedAt: 1800000000000,
    baseRef: "origin/master",
    repos: [
      {
        repoName: "yl-cwhsea-wms-api",
        projectPath: "~/Developer/company/WMS/yl-cwhsea-wms-api",
        branch: "hevin.yang/feature/WMS-016-shop-query-unify",
        resolvedTargetRef: "hevin.yang/feature/WMS-016-shop-query-unify",
        baseRef: "origin/master",
        dirty: false,
        baseUpdate: {
          ok: true,
          remote: "origin",
          remoteBranch: "master",
          localBranch: "master",
          steps: [],
        },
        commits: ["abc1234 feat: shop query unify"],
        files: [
          {
            path: "src/main/java/com/demo/ShopController.java",
            status: "M",
            additions: 12,
            deletions: 3,
            riskTags: ["入口/API"],
          },
        ],
        additions: 12,
        deletions: 3,
        diff: "diff --git a/x b/x",
        diffTruncated: false,
        warnings: [],
      },
    ],
    verdict: {
      status: "approved",
      reviewer: "Hevin",
      summary,
      items: ["无阻塞项"],
      updatedAt: 1800000000000,
    },
  }
}

test("resolveCodeReviewProjectPath follows categorized workspace migration", () => {
  const root = mkdtempSync(join(tmpdir(), "agent-panel-review-path-"))
  const repoPath = join(root, "backend", "demo-api")
  mkdirSync(repoPath, { recursive: true })
  assert.equal(resolveCodeReviewProjectPath(join(root, "demo-api"), "demo-api"), repoPath)
})

test("parseUnifiedDiff splits files, hunks, and line numbers", () => {
  const parsed = parseUnifiedDiff([
    "diff --git a/src/Old.java b/src/New.java",
    "similarity index 90%",
    "rename from src/Old.java",
    "rename to src/New.java",
    "--- a/src/Old.java",
    "+++ b/src/New.java",
    "@@ -10,3 +10,4 @@ class Demo {",
    " context();",
    "-removed();",
    "+added();",
    "+addedAgain();",
    " unchanged();",
    "\\ No newline at end of file",
  ].join("\n"))

  assert.equal(parsed.length, 1)
  assert.equal(parsed[0].path, "src/New.java")
  assert.equal(parsed[0].hunks.length, 1)
  assert.deepEqual(parsed[0].hunks[0].lines, [
    { kind: "context", oldLine: 10, newLine: 10, content: "context();" },
    { kind: "deletion", oldLine: 11, newLine: null, content: "removed();" },
    { kind: "addition", oldLine: null, newLine: 11, content: "added();" },
    { kind: "addition", oldLine: null, newLine: 12, content: "addedAgain();" },
    { kind: "context", oldLine: 12, newLine: 13, content: "unchanged();" },
    { kind: "meta", oldLine: null, newLine: null, content: "\\ No newline at end of file" },
  ])
})

test("parseUnifiedDiff handles deleted files", () => {
  const parsed = parseUnifiedDiff([
    "diff --git a/src/Removed.java b/src/Removed.java",
    "--- a/src/Removed.java",
    "+++ /dev/null",
    "@@ -1 +0,0 @@",
    "-removed",
  ].join("\n"))
  assert.equal(parsed[0].path, "src/Removed.java")
  assert.equal(parsed[0].hunks[0].lines[0].kind, "deletion")
})

test("classifyCodeReviewRiskTags detects common WMS risk surfaces", () => {
  assert.deepEqual(
    classifyCodeReviewRiskTags("src/main/java/com/demo/ShopController.java", "M", 10, 2),
    ["入口/API"],
  )
  assert.ok(classifyCodeReviewRiskTags("src/main/resources/mapper/ShipmentMapper.xml", "M", 5, 1).includes("DB"))
  assert.ok(classifyCodeReviewRiskTags("src/main/resources/application.yml", "M", 5, 1).includes("配置"))
  assert.ok(classifyCodeReviewRiskTags("src/main/java/com/demo/RocketMqConsumer.java", "D", 500, 10).includes("删除"))
  assert.ok(classifyCodeReviewRiskTags("src/main/java/com/demo/RocketMqConsumer.java", "D", 500, 10).includes("大改动"))
})

test("upsertCodeReviewBlock appends managed block when review.md has none", () => {
  const out = upsertCodeReviewBlock("# Review\n\n历史内容\n", sampleSnapshot())
  assert.ok(out.includes("历史内容"))
  assert.ok(out.includes(CODE_REVIEW_BLOCK_START))
  assert.ok(out.includes(CODE_REVIEW_BLOCK_END))
  assert.ok(out.includes("- 结论：通过"))
  assert.ok(out.includes("人工确认通过"))
})

test("upsertCodeReviewBlock replaces only the managed block", () => {
  const existing = [
    "# Review",
    "",
    "保留在前面",
    "",
    CODE_REVIEW_BLOCK_START,
    "旧 block",
    CODE_REVIEW_BLOCK_END,
    "",
    "保留在后面",
    "",
  ].join("\n")
  const out = upsertCodeReviewBlock(existing, sampleSnapshot("新的 Review 摘要"))
  assert.ok(out.includes("保留在前面"))
  assert.ok(out.includes("保留在后面"))
  assert.ok(out.includes("新的 Review 摘要"))
  assert.ok(!out.includes("旧 block"))
})
