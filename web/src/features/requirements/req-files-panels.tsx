import { useEffect, useMemo, useState } from "react"
import type { Requirement, RequirementAttachment, RequirementAttachmentsPayload } from "../../types"
import { useFetch } from "../../lib/api"
import { LoadingCard, PanelHead } from "../../components/ui"

const TEXT_ATTACHMENT_EXTS = new Set(["sql", "txt", "md", "yaml", "yml", "json", "csv"])

function attachmentHumanBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`
}

function attachmentFormatTime(ms: number): string {
  if (!ms) return "-"
  const d = new Date(ms)
  const p = (n: number) => String(n).padStart(2, "0")
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}`
}

export function RequirementAttachmentsPanel({ req }: { req: Requirement }) {
  // full=1：后端返回附件全文（上限 200KB）；双栏查看器左列表、右全文，左右独立滚动。
  const { data, error, loading } = useFetch<RequirementAttachmentsPayload>(
    req.reqDir ? `/api/requirement/attachments?id=${encodeURIComponent(req.id)}&full=1` : null,
    [req.id],
  )
  const rows = useMemo(() => data?.attachments ?? [], [data])
  const previewable = (row: RequirementAttachment) => TEXT_ATTACHMENT_EXTS.has(row.extension) && Boolean((row.sample || "").trim())
  const [selected, setSelected] = useState<string | null>(null)
  // 默认选中第一个可预览附件，进入即可读；选中项被删除/刷新后兜底回第一个。
  useEffect(() => {
    if (selected && rows.some((row) => row.filename === selected)) return
    setSelected(rows.find((row) => previewable(row))?.filename ?? rows[0]?.filename ?? null)
  }, [rows])
  const [copiedKey, setCopiedKey] = useState<string | null>(null)
  const copyText = async (key: string, text: string) => {
    try {
      await navigator.clipboard.writeText(text)
      setCopiedKey(key)
      window.setTimeout(() => setCopiedKey((k) => (k === key ? null : k)), 1600)
    } catch {
      setCopiedKey(null)
    }
  }
  const active = rows.find((row) => row.filename === selected) ?? null
  const activePreviewable = active ? previewable(active) : false
  const activeBadge = active ? (active.summary.length ? active.summary.join(" / ") : active.extension || "-") : ""
  const chip = loading ? "loading" : error ? "error" : `${rows.length} 个文件`
  return <section id="attachments" className="react-panel react-attachments-panel"><PanelHead kicker="Attachments" title="附件" chip={chip} />
    <p className="react-muted">展示需求目录 <code>attachments/</code> 下的非代码资产（排查数据、SQL、截图、导出件等），与上线清单相互独立；左侧选择附件（名称右侧可一键复制内容/路径），右侧展示全文，附件多、内容大时左右独立滚动。</p>
    {error ? <p className="react-effort-error">附件加载失败：{error}</p> : loading ? <LoadingCard label="正在加载附件…" /> : rows.length === 0 ? <p className="react-muted">暂无附件。</p> : <div className="react-att-layout">
      <div className="react-att-list">{rows.map((row) => <div key={row.filename} role="button" className={`react-att-item${row.filename === selected ? " react-att-active" : ""}`} onClick={() => setSelected(row.filename)}>
        <span className="react-att-item-name" title={row.filename}>{row.filename}</span>
        <span className="react-att-item-actions" onClick={(e) => e.stopPropagation()}>
          {previewable(row) ? <button type="button" className="react-copy-link-btn" title="复制附件全文内容" onClick={() => copyText(`content:${row.filename}`, row.sample)}>{copiedKey === `content:${row.filename}` ? "✓" : "内容"}</button> : null}
          <button type="button" className="react-copy-link-btn" title="复制附件文件路径" onClick={() => copyText(`path:${row.filename}`, row.path)}>{copiedKey === `path:${row.filename}` ? "✓" : "路径"}</button>
        </span>
      </div>)}</div>
      <div className="react-att-main">{active ? <>
        <div className="react-att-main-head"><strong>{active.filename}</strong><span className="react-attachment-badge">{activeBadge}</span><span className="react-attachment-size">{attachmentHumanBytes(active.size)} · {attachmentFormatTime(active.mtime)}</span><span className="react-att-main-actions">
          {activePreviewable ? <button type="button" className="react-copy-link-btn" title="复制附件全文内容" onClick={() => copyText(`main-content:${active.filename}`, active.sample)}>{copiedKey === `main-content:${active.filename}` ? "✓ 已复制" : "复制内容"}</button> : null}
          <button type="button" className="react-copy-link-btn" title="复制附件在需求目录下的完整路径" onClick={() => copyText(`main-path:${active.filename}`, active.path)}>{copiedKey === `main-path:${active.filename}` ? "✓ 已复制" : "复制路径"}</button>
        </span></div>
        <code className="react-attachment-path">{active.path}</code>
        {activePreviewable ? <pre>{active.sample}</pre> : <p className="react-muted">该文件为二进制或不可预览内容，请按路径打开核对。</p>}
      </> : <p className="react-muted">在左侧选择一个附件查看全文。</p>}</div>
    </div>}
  </section>
}

export function RequirementFilesPanel({ req }: { req: Requirement }) {
  const corePath = (file: string, path?: string) => path || (req.reqDir ? `${req.reqDir}/${file}` : "-")
  const existing = (file: string, path?: string): [string, string] | null => path ? [file, path] : null
  const groups: { title: string; note: string; rows: [string, string][] }[] = [
    { title: "核心文档", note: "新需求默认创建，Agent 执行过程中持续维护。", rows: [
      ["meta.md", corePath("meta.md", req.metaPath)],
      ["background.md", corePath("background.md", req.backgroundPath)],
      ["technical-plan.md", corePath("technical-plan.md", req.technicalPlanPath)],
      ["notes.md", corePath("notes.md", req.notesPath)],
    ] },
    { title: "按需阶段文件", note: "进入自测、发布、审查、经验总结等阶段后再创建。", rows: [
      existing("test.md", req.testPath),
      existing("test-scenario.md", req.testScenarioPath),
      existing("release-manifest.md", req.releaseManifestPath),
      existing("release-check.md", req.releaseCheckPath),
      existing("experience-summary.md", req.experienceSummaryPath),
      existing("troubleshooting.md", req.troubleshootingPath),
      existing("review.md", req.reviewPath),
    ].filter(Boolean) as [string, string][] },
    { title: "历史兼容文件", note: "旧需求存在时继续读取；新需求不再默认要求。", rows: [
      existing("alignment.md", req.alignmentPath),
      existing("impact.md", req.impactPath),
      existing("memory.md", req.memoryPath),
      existing("branch.md", req.branchPath),
      existing("config-changes.md", req.configPath),
    ].filter(Boolean) as [string, string][] },
  ]
  return <section className="react-panel"><PanelHead kicker="Files" title="需求文件" />
    {groups.map((group) => <div key={group.title} className="react-file-group"><strong>{group.title}</strong><p className="react-muted">{group.note}</p>{group.rows.length ? <div className="react-meta-grid">{group.rows.flatMap(([name, value]) => [<span key={`${group.title}-${name}-n`}>{name}</span>, <span key={`${group.title}-${name}-v`}>{value}</span>])}</div> : <p className="react-muted">暂无已创建文件。</p>}</div>)}
  </section>
}
