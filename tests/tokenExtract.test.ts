/**
 * tests/tokenExtract.test.ts
 *
 * Unit tests for the pure curl-token extractor.
 */
import { describe, it } from "node:test"
import assert from "node:assert/strict"
import { extractTokensFromCurl } from "../src/tokenExtract.ts"

describe("extractTokensFromCurl", () => {
  it("extracts YLOPS_TOKEN from Authorization Bearer header", () => {
    const curl = `curl 'https://ylops.jtexpress.com.cn/api/user/profile/info/' -H 'Authorization: Bearer eyJ0eXAi.eyJ0b2tlbl90eXBlIjoiYWNjZXNzIn0.signature'`
    const tokens = extractTokensFromCurl(curl)
    assert.equal(tokens.length, 1)
    assert.equal(tokens[0].name, "YLOPS_TOKEN")
    assert.equal(tokens[0].value, "eyJ0eXAi.eyJ0b2tlbl90eXBlIjoiYWNjZXNzIn0.signature")
    assert.equal(tokens[0].source, "Authorization header")
    assert.equal(tokens[0].file, "secrets")
  })

  it("extracts YLOPS_TOKEN from visionToken cookie when no Authorization header", () => {
    const curl = `curl 'https://ylops.jtexpress.com.cn/' -H 'Cookie: _pk_id=abc; visionToken=eyJaccess.jwt.token; other=xyz'`
    const tokens = extractTokensFromCurl(curl)
    assert.equal(tokens.length, 1)
    assert.equal(tokens[0].name, "YLOPS_TOKEN")
    assert.equal(tokens[0].value, "eyJaccess.jwt.token")
    assert.equal(tokens[0].source, "visionToken cookie")
  })

  it("extracts YLOPS_REFRESH_TOKEN from visionRefresh cookie", () => {
    const curl = `curl 'https://ylops.jtexpress.com.cn/' -H 'Authorization: Bearer eyJaccess.jwt.token' -H 'Cookie: visionToken=eyJaccess.jwt.token; visionRefresh=eyJrefresh.jwt.token'`
    const tokens = extractTokensFromCurl(curl)
    assert.equal(tokens.length, 2)
    const refresh = tokens.find((t) => t.name === "YLOPS_REFRESH_TOKEN")
    assert.ok(refresh)
    assert.equal(refresh!.value, "eyJrefresh.jwt.token")
    assert.equal(refresh!.source, "visionRefresh cookie")
  })

  it("prefers Authorization header over visionToken cookie", () => {
    const curl = `curl 'https://ylops.jtexpress.com.cn/' -H 'Authorization: Bearer fromAuthHeader' -H 'Cookie: visionToken=fromCookie'`
    const tokens = extractTokensFromCurl(curl)
    const access = tokens.find((t) => t.name === "YLOPS_TOKEN")
    assert.equal(access?.value, "fromAuthHeader")
    assert.equal(access?.source, "Authorization header")
  })

  it("returns empty array when no recognised tokens", () => {
    const curl = `curl 'https://example.com/' -H 'Accept: text/html'`
    const tokens = extractTokensFromCurl(curl)
    assert.equal(tokens.length, 0)
  })

  it("returns empty array for empty input", () => {
    assert.deepEqual(extractTokensFromCurl(""), [])
    assert.deepEqual(extractTokensFromCurl("   "), [])
  })

  it("handles multi-line curl with line continuations", () => {
    const curl = [
      `curl 'https://ylops.jtexpress.com.cn/api/app/service/' \\`,
      `  --compressed \\`,
      `  -H 'Authorization: Bearer multi.line.jwt' \\`,
      `  -H 'Cookie: _pk_id=123; visionToken=multi.line.jwt; visionRefresh=refresh.multi.jwt'`,
    ].join("\n")
    const tokens = extractTokensFromCurl(curl)
    assert.equal(tokens.length, 2)
    assert.equal(tokens[0].name, "YLOPS_TOKEN")
    assert.equal(tokens[1].name, "YLOPS_REFRESH_TOKEN")
  })
})
