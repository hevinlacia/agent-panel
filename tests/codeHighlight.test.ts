/**
 * Tests for `src/codeHighlight.ts`.
 *
 * Covers:
 *   - detectHighlightLanguage: extension + filename mapping, unknown -> null
 *   - highlightDiffLines: per-line alignment, multi-line comment rebalancing,
 *     HTML escaping of generics/ampersands, meta lines excluded, fallbacks
 */

import { test } from "node:test"
import { strict as assert } from "node:assert"

import { detectHighlightLanguage, highlightDiffLines } from "../src/codeHighlight.ts"

test("detectHighlightLanguage maps common extensions", () => {
  assert.equal(detectHighlightLanguage("src/main/Foo.java"), "java")
  assert.equal(detectHighlightLanguage("app.ts"), "typescript")
  assert.equal(detectHighlightLanguage("app.tsx"), "tsx")
  assert.equal(detectHighlightLanguage("config/application.yml"), "yaml")
  assert.equal(detectHighlightLanguage("Mapper.xml"), "xml")
  assert.equal(detectHighlightLanguage("Dockerfile"), "dockerfile")
  assert.equal(detectHighlightLanguage("build.gradle"), "groovy")
})

test("detectHighlightLanguage returns null for unknown", () => {
  assert.equal(detectHighlightLanguage("README"), null)
  assert.equal(detectHighlightLanguage("foo.unknownext"), null)
})

test("highlightDiffLines returns empty array for empty input", () => {
  assert.deepEqual(highlightDiffLines([], "java"), [])
})

test("highlightDiffLines falls back to escaped text when lang is null", () => {
  const out = highlightDiffLines(
    [{ kind: "context", content: "List<String> & x" }],
    null,
  )
  assert.equal(out.length, 1)
  assert.equal(out[0], "List&lt;String&gt; &amp; x")
  // No span tags in fallback.
  assert.ok(!out[0].includes("<span"))
})

test("highlightDiffLines wraps tokens in hljs spans", () => {
  const out = highlightDiffLines(
    [{ kind: "addition", content: "public class Foo {}" }],
    "java",
  )
  assert.equal(out.length, 1)
  assert.ok(out[0].includes("hljs-keyword"), `expected hljs-keyword in: ${out[0]}`)
  assert.ok(out[0].includes("hljs-title"), `expected hljs-title in: ${out[0]}`)
})

test("highlightDiffLines escapes generics so HTML stays valid", () => {
  const out = highlightDiffLines(
    [{ kind: "addition", content: "List<String> items" }],
    "java",
  )
  assert.equal(out.length, 1)
  // The raw < of the generic must be escaped, not emitted as a tag.
  assert.ok(!out[0].includes("<String>"), `unescaped generic in: ${out[0]}`)
  assert.ok(out[0].includes("&lt;String&gt;"), `expected escaped generic in: ${out[0]}`)
})

test("highlightDiffLines keeps block comments colored across lines", () => {
  const lines = [
    { kind: "context", content: "  /* multi" },
    { kind: "context", content: "     line" },
    { kind: "context", content: "     comment */" },
  ]
  const out = highlightDiffLines(lines, "java")
  assert.equal(out.length, 3)
  // Every line of the block comment must carry the comment class so the
  // color is continuous, even though highlight.js emits one spanning span.
  for (let i = 0; i < 3; i++) {
    assert.ok(
      out[i].includes("hljs-comment"),
      `line ${i} missing hljs-comment: ${out[i]}`,
    )
  }
  // Each line must be balanced HTML (open/close spans match).
  for (let i = 0; i < 3; i++) {
    const opens = (out[i].match(/<span/g) || []).length
    const closes = (out[i].match(/<\/span>/g) || []).length
    assert.equal(opens, closes, `line ${i} unbalanced spans: ${out[i]}`)
  }
})

test("highlightDiffLines aligns output one-to-one with input lines", () => {
  const lines = [
    { kind: "context", content: "int a = 1;" },
    { kind: "addition", content: "int b = 2;" },
    { kind: "deletion", content: "int c = 3;" },
  ]
  const out = highlightDiffLines(lines, "java")
  assert.equal(out.length, 3)
  // Each line should contain a number literal (2 -> 3, but at least one
  // number token per line).
  for (let i = 0; i < 3; i++) {
    assert.ok(out[i].includes("hljs-number"), `line ${i} missing number: ${out[i]}`)
  }
})

test("highlightDiffLines excludes meta lines from tokenization", () => {
  const lines = [
    { kind: "context", content: "int a = 1;" },
    { kind: "meta", content: "\\ No newline at end of file" },
  ]
  const out = highlightDiffLines(lines, "java")
  assert.equal(out.length, 2)
  // Meta line is escaped plain text, no code spans.
  assert.ok(!out[1].includes("<span"), `meta line got spans: ${out[1]}`)
  assert.ok(out[1].includes("No newline"), `meta content lost: ${out[1]}`)
})

test("highlightDiffLines handles empty content lines", () => {
  const lines = [
    { kind: "context", content: "int a = 1;" },
    { kind: "context", content: "" },
    { kind: "context", content: "int b = 2;" },
  ]
  const out = highlightDiffLines(lines, "java")
  assert.equal(out.length, 3)
  // Middle empty line stays a (possibly empty) fragment, not undefined.
  assert.equal(typeof out[1], "string")
})
