/**
 * Tests for src/codeReviewJob.ts prompt loading.
 *
 * `buildCodeReviewAiPrompt` loads the editable Markdown template at
 * `prompts/code-review-ai.md` (repo-relative, resolved from the module) and
 * substitutes the per-requirement tokens. These tests pin that contract:
 * the template exists, the tokens are replaced, and the key review
 * instructions (materials + logic/performance angles) are present.
 */

import { test } from "node:test"
import { strict as assert } from "node:assert"

import { buildCodeReviewAiPrompt, CODE_REVIEW_AI_FILE } from "../src/codeReviewJob.ts"
import type { Requirement } from "../src/requirements.ts"

function fakeReq(overrides: Partial<Pick<Requirement, "id" | "title" | "reqDir">> = {}): Requirement {
  return { id: "WMS-TEST", title: "测试需求", reqDir: "/tmp/req/WMS-TEST", ...overrides } as unknown as Requirement
}

test("buildCodeReviewAiPrompt loads the template and substitutes tokens", async () => {
  const prompt = await buildCodeReviewAiPrompt(fakeReq({ id: "WMS-018", title: "订单回退", reqDir: "/tmp/req/WMS-018" }))
  assert.ok(prompt.includes("WMS-018 - 订单回退"), "header carries id + title")
  assert.ok(prompt.includes("/tmp/req/WMS-018"), "req dir substituted")
  assert.ok(prompt.includes(CODE_REVIEW_AI_FILE), "output filename substituted")
  // No leftover template tokens.
  assert.equal(/\{\{[A-Z_]+\}\}/.test(prompt), false, "no unsubstituted tokens")
})

test("buildCodeReviewAiPrompt carries the materials + logic/performance angles", async () => {
  const prompt = await buildCodeReviewAiPrompt(fakeReq())
  assert.ok(prompt.includes("审查材料"))
  assert.ok(prompt.includes("扩大阅读"))
  assert.ok(prompt.includes("评估角度"))
  assert.ok(prompt.includes("[逻辑]"))
  assert.ok(prompt.includes("[性能]"))
  assert.ok(prompt.includes("3 秒"))
  assert.ok(prompt.includes("审查概览"))
})

test("buildCodeReviewAiPrompt falls back to 未知 when reqDir is absent", async () => {
  const prompt = await buildCodeReviewAiPrompt(fakeReq({ reqDir: undefined }))
  assert.ok(prompt.includes("需求目录：未知"))
})
