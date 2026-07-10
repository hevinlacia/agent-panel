/**
 * Tests for src/branchScope.ts.
 *
 * Covers:
 *   - readBranchScope: well-formed JSON, missing file, malformed JSON,
 *     missing/empty repos array.
 *   - fallbackFromBranchMd: the five real-world branch.md formats seen
 *     in ~/.agents/req/WMS/* (multi-repo sections, single-repo flat
 *     tables, `## 仓库: <name>` sections, `| 仓库 | 分支 |` tables, and
 *     list-block `- 仓库：x` / `- 分支：y`).
 *
 * The fallback assertions are strict on repo + branch pairing,
 * which is the user-facing "which app got which branch" guarantee.
 */

import { test } from "node:test"
import { strict as assert } from "node:assert"
import { mkdtempSync, writeFileSync, rmSync } from "node:fs"
import { tmpdir } from "node:os"
import { join } from "node:path"

import {
  readBranchScope,
  fallbackFromBranchMd,
  BRANCH_SCOPE_FILE,
  type BranchRepo,
} from "../src/branchScope.ts"

function findRepo(repos: BranchRepo[], name: string): BranchRepo | undefined {
  return repos.find((r) => r.repoName === name)
}

// ---------------------------------------------------------------------------
// readBranchScope
// ---------------------------------------------------------------------------

test("readBranchScope: parses a well-formed branches.json (v2)", async () => {
  const dir = mkdtempSync(join(tmpdir(), "bs-"))
  writeFileSync(
    join(dir, BRANCH_SCOPE_FILE),
    JSON.stringify({
      version: 2,
      updatedAt: 1750000000000,
      repos: [
        {
          repoName: "yl-cwhsea-wms-api",
          role: "后端",
          path: "~/dev/yl-cwhsea-wms-api/",
          branches: ["hevin.yang/feature/x"],
        },
      ],
    }),
  )
  const scope = await readBranchScope(dir)
  assert.ok(scope)
  assert.equal(scope!.repos.length, 1)
  const r = scope!.repos[0]
  assert.equal(r.repoName, "yl-cwhsea-wms-api")
  assert.equal(r.role, "后端")
  assert.equal(r.path, "~/dev/yl-cwhsea-wms-api/")
  assert.deepEqual(r.branches, ["hevin.yang/feature/x"])
  rmSync(dir, { recursive: true, force: true })
})

test("readBranchScope: reads v1 projectPath as path (backwards compat)", async () => {
  const dir = mkdtempSync(join(tmpdir(), "bs-"))
  writeFileSync(
    join(dir, BRANCH_SCOPE_FILE),
    JSON.stringify({
      version: 1,
      updatedAt: 1,
      repos: [
        {
          repoName: "yl-cwhsea-wms-api",
          projectPath: "~/dev/wms-api/",
          branches: ["feature/x"],
          merge: { test: "merged" },
        },
      ],
    }),
  )
  const scope = await readBranchScope(dir)
  assert.ok(scope)
  assert.equal(scope!.repos[0].path, "~/dev/wms-api/")
  rmSync(dir, { recursive: true, force: true })
})

test("readBranchScope: returns null when file is missing", async () => {
  const dir = mkdtempSync(join(tmpdir(), "bs-"))
  assert.equal(await readBranchScope(dir), null)
  rmSync(dir, { recursive: true, force: true })
})

test("readBranchScope: returns null on malformed JSON", async () => {
  const dir = mkdtempSync(join(tmpdir(), "bs-"))
  writeFileSync(join(dir, BRANCH_SCOPE_FILE), "{not json")
  assert.equal(await readBranchScope(dir), null)
  rmSync(dir, { recursive: true, force: true })
})

test("readBranchScope: returns null when repos array is empty", async () => {
  const dir = mkdtempSync(join(tmpdir(), "bs-"))
  writeFileSync(join(dir, BRANCH_SCOPE_FILE), JSON.stringify({ version: 1, updatedAt: 1, repos: [] }))
  assert.equal(await readBranchScope(dir), null)
  rmSync(dir, { recursive: true, force: true })
})

// ---------------------------------------------------------------------------
// fallbackFromBranchMd - real-world formats
// ---------------------------------------------------------------------------

const WMS018 = `# WMS-018 Branches

## 后端仓库

| Item | Value |
| --- | --- |
| Source branch | origin/master |
| Project path | ~/Developer/company/WMS/yl-cwhsea-wms-outbound-api/ |
| 需求分支 | \`hevin.yang/feature/WMS-018-order-rollback\` |
| test merge | 已 merge 到 test 并 push |
| UAT merge | 已 merge 到 UAT-2607 并 push |

## 前端仓库

| Item | Value |
| --- | --- |
| Project path | ~/Developer/company/WMS/yl-cwhsea-wms-web-front/ |
| 需求分支 | \`hevin.yang/feature/WMS-018-order-rollback\` |
| test merge | 已 merge 到 test |
| UAT merge | 已 merge 到 UAT-2606 |

## BFF 仓库（yl-cwhsea-wms-web）

| Item | Value |
| --- | --- |
| Project path | ~/Developer/company/WMS/yl-cwhsea-wms-web/ |
| 需求分支 | \`hevin.yang/feature/WMS-018-order-rollback\` |
| test merge | 已 merge 到 test |

## Commit / Diff Notes
- worktree 子分支: \`hevin.yang/feature/WMS-018-order-rollback--impl\`（旧，已失效）
`

test("fallback: WMS-018 multi-repo sections (后端/前端/BFF)", () => {
  const repos = fallbackFromBranchMd(WMS018)
  const names = repos.map((r) => r.repoName).sort()
  assert.deepEqual(names, ["yl-cwhsea-wms-outbound-api", "yl-cwhsea-wms-web", "yl-cwhsea-wms-web-front"])
  for (const r of repos) {
    assert.deepEqual(r.branches, ["hevin.yang/feature/WMS-018-order-rollback"])
  }
  // The stale --impl branch from the notes section must NOT leak in.
  assert.ok(!repos.some((r) => r.branches.includes("hevin.yang/feature/WMS-018-order-rollback--impl")))
  // Roles parsed from section titles.
  assert.equal(findRepo(repos, "yl-cwhsea-wms-outbound-api")?.role, "后端")
  assert.equal(findRepo(repos, "yl-cwhsea-wms-web-front")?.role, "前端")
  assert.equal(findRepo(repos, "yl-cwhsea-wms-web")?.role, "BFF")
})

const WMS011 = `# WMS-011 Branches

| Item | Value |
| --- | --- |
| Source branch | \`hevin.yang/feature/WMS-011-return-order-delay-rocketmq\` |
| Target branch | master |
| Project path | ~/Developer/company/WMS/yl-cwhsea-wms-api |
| Merge status | 已合并到 test + UAT-2607，已 push |

## 部署状态

| 环境 | 分支 | 状态 |
| --- | --- | --- |
| test | test | ✅ 已部署 |
| SEA UAT | UAT-2607 | 🔄 构建中 |
`

test("fallback: WMS-011 single-repo flat Item|Value table", () => {
  const repos = fallbackFromBranchMd(WMS011)
  assert.equal(repos.length, 1)
  assert.equal(repos[0].repoName, "yl-cwhsea-wms-api")
  assert.deepEqual(repos[0].branches, ["hevin.yang/feature/WMS-011-return-order-delay-rocketmq"])
  // master is a base branch and must not appear as a feature branch.
  assert.ok(!repos[0].branches.includes("master"))
})

const WMS014 = `# Branch

## 仓库: yl-cwhsea-wms-outbound-api

| 分支 | 说明 |
|------|------|
| \`hevin.yang/feature/whole-order-allocation\` | 需求分支，从 origin/master 创建 |
| \`test\` | 已合并 |
| \`UAT-2607\` | 已合并 |

### 合并状态

- [x] 合并到 test 分支
- [x] 合并到 UAT 分支
- [ ] 合并到 master 分支
`

test("fallback: WMS-014 `## 仓库: <name>` section with branch-first table", () => {
  const repos = fallbackFromBranchMd(WMS014)
  assert.equal(repos.length, 1)
  assert.equal(repos[0].repoName, "yl-cwhsea-wms-outbound-api")
  assert.deepEqual(repos[0].branches, ["hevin.yang/feature/whole-order-allocation"])
  // Base branches in the table are not feature branches.
  assert.ok(!repos[0].branches.includes("test"))
  assert.ok(!repos[0].branches.includes("UAT-2607"))
})

const WMS005 = `# WMS-005 分支信息

## 分支

| 仓库 | 分支 | 说明 |
|------|------|------|
| yl-cwhsea-wms-plus-api | \`hevin.yang/fix/outboundv2-oplog-rocketmq-batch\` | 消费端优化 |
| yl-cwhsea-wms-plus-api | \`UAT-2607\` | UAT 发版分支 |

## 合并状态

| 目标分支 | 状态 |
|---------|------|
| test | ✅ 已合并并推送 |
| master | ❌ 待合并 |
`

test("fallback: WMS-005 `| 仓库 | 分支 |` repo-column table", () => {
  const repos = fallbackFromBranchMd(WMS005)
  assert.equal(repos.length, 1)
  assert.equal(repos[0].repoName, "yl-cwhsea-wms-plus-api")
  assert.deepEqual(repos[0].branches, ["hevin.yang/fix/outboundv2-oplog-rocketmq-batch"])
  assert.ok(!repos[0].branches.includes("UAT-2607"))
})

const WMS001 = `# 分支信息

## 需求主分支
- 仓库：\`yl-cwhsea-wms-system-api\`
- 分支：\`yhw/【重构】日志功能\`
- 已合并到 \`test\`，已推送
- 已合并到 \`UAT-2607\`

## 活跃 Fix 分支（基于 master）

| 仓库 | 分支 | 基于 | commit | 说明 | 状态 |
|------|------|------|--------|------|------|
| plus-api | \`hevin.yang/fix/createwave-last-updated-by\` | master | 133c7b431 | 波次创建 | 已合并到 test + UAT-2607 |
| outbound-api | \`hevin.yang/fix/add-to-wave-last-updated-by\` | master | 20f0c263 | 加入波次 | 已合并到 test + UAT-2607 |

## 历史 Fix 分支（已合并到主分支，仅保留归档）

| 分支 | fix commit | 说明 |
|------|-----------|------|
| \`hevin.yang/fix/shipment-update-log\` | 2ea9b5fb | ShipmentHeader diff |

## 跨仓库分支
- \`yl-cwhsea-wms-components\` 分支 \`yhw/业务日志记录通用模块\`
`

test("fallback: WMS-001 list-block + repo-column table + archived skip", () => {
  const repos = fallbackFromBranchMd(WMS001)
  const byName = new Map(repos.map((r) => [r.repoName, r]))
  // Main branch repo from list block.
  assert.ok(byName.has("yl-cwhsea-wms-system-api"))
  assert.deepEqual(byName.get("yl-cwhsea-wms-system-api")!.branches, ["yhw/【重构】日志功能"])
  // Two fix repos, short names expanded to full repo names.
  assert.ok(byName.has("yl-cwhsea-wms-plus-api"))
  assert.deepEqual(byName.get("yl-cwhsea-wms-plus-api")!.branches, ["hevin.yang/fix/createwave-last-updated-by"])
  assert.ok(byName.has("yl-cwhsea-wms-outbound-api"))
  // Cross-repo component.
  assert.ok(byName.has("yl-cwhsea-wms-components"))
  assert.deepEqual(byName.get("yl-cwhsea-wms-components")!.branches, ["yhw/业务日志记录通用模块"])
  // Archived fix branch must be dropped.
  assert.ok(!repos.some((r) => r.branches.includes("hevin.yang/fix/shipment-update-log")))
})

test("fallback: empty / whitespace input returns []", () => {
  assert.deepEqual(fallbackFromBranchMd(""), [])
  assert.deepEqual(fallbackFromBranchMd("   \n  "), [])
})

const WMS016 = `# WMS-016 Branches

| Item | Value |
| --- | --- |
| Source branch | origin/master |
| Target branch | test (部署测试环境时合并) |
| Project path | ~/Developer/company/WMS/yl-cwhsea-wms-web-front/ |
| Requirement branch | hevin.yang/feature/WMS-016-shop-query-unify |
| Merge status | 已合 master（merge c698dca9）；UAT-2606 merge e3e374e2/e0df2950 |
`

test("fallback: WMS-016 single-repo with un-backticked branch value", () => {
  const repos = fallbackFromBranchMd(WMS016)
  assert.equal(repos.length, 1)
  assert.equal(repos[0].repoName, "yl-cwhsea-wms-web-front")
  // The branch is NOT backticked; it must be rescued by the table-cell
  // label detector (| Requirement branch | <value> |).
  assert.deepEqual(repos[0].branches, ["hevin.yang/feature/WMS-016-shop-query-unify"])
  // Stray `/` between two backtick pairs (e3e374e2/e0df2950) must not
  // leak in as a bogus branch.
  assert.ok(!repos[0].branches.includes("/"))
})
