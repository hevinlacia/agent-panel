import { Copy, Library } from "lucide-react"
import { useMemo, useState } from "react"
import type { Requirement, RequirementDocPayload } from "../../types"
import { useFetch } from "../../lib/api"
import { LoadingCard, PanelHead } from "../../components/ui"

function markdownPreview(content: string, expanded: boolean, max = 2600): { text: string; truncated: boolean } {
  const clean = (content || "").trim()
  if (!clean) return { text: "", truncated: false }
  if (expanded || clean.length <= max) return { text: clean, truncated: false }
  return { text: `${clean.slice(0, max)}\n…`, truncated: true }
}

export function RequirementDocPanel({ id, req, docType, title, kicker, description, path, actions, jumpOnly }: { id: string; req: Requirement; docType: string; title: string; kicker: string; description: string; path?: string; actions?: React.ReactNode; jumpOnly?: boolean }) {
  const [expanded, setExpanded] = useState(false)
  const doc = useFetch<RequirementDocPayload>(!jumpOnly && req.reqDir ? `/api/requirement/doc?id=${encodeURIComponent(req.id)}&file=${encodeURIComponent(docType)}` : null, [req.id, docType])
  const content = doc.data?.content || ""
  const preview = markdownPreview(content, expanded)
  const chip = jumpOnly ? "点击查看全文" : doc.loading ? "loading" : doc.data?.exists ? doc.data.file : path ? "path only" : "missing"
  const shownPath = jumpOnly ? path : doc.data?.path || path
  return <section id={id} className="react-panel react-doc-panel"><PanelHead kicker={kicker} title={title} chip={chip} />
    <p className="react-muted">{description}</p>
    <div className="react-actions">{actions}{shownPath ? <code className="react-doc-path">{shownPath}</code> : null}</div>
    {jumpOnly ? null : <>{doc.error ? <p className="react-effort-error">加载失败：{doc.error}</p> : doc.loading ? <LoadingCard label="正在加载文档…" /> : preview.text ? <><pre className="react-doc-preview">{preview.text}</pre>{preview.truncated || expanded ? <div className="react-actions"><button type="button" onClick={() => setExpanded((v) => !v)}>{expanded ? "收起" : "展开全文"}</button></div> : null}</> : <p className="react-muted">暂无内容。可在需求澄清阶段生成或更新该文档。</p>}</>}
  </section>
}

/** 关联 PRD 面板：需求目录存在 prd.md（目前为飞书 PRD 存档）时，从存档提取来源链接展示，右侧一键复制。 */
export function RequirementPrdPanel({ req }: { req: Requirement }) {
  const doc = useFetch<RequirementDocPayload>(req.prdPath ? `/api/requirement/doc?id=${encodeURIComponent(req.id)}&doc=prd` : null, [req.id])
  const [copied, setCopied] = useState(false)
  const link = useMemo(() => (doc.data?.content || "").match(/https?:\/\/[^\s)\]}"'，。；』」]+/)?.[0] || "", [doc.data])
  const copyLink = async () => {
    if (!link) return
    try {
      await navigator.clipboard.writeText(link)
      setCopied(true)
      window.setTimeout(() => setCopied(false), 1600)
    } catch {
      setCopied(false)
    }
  }
  return <section id="prd" className="react-panel react-doc-panel"><PanelHead kicker="PRD" title="关联 PRD" chip={doc.loading ? "loading" : link ? "飞书文档" : "prd.md"} />
    <p className="react-muted">需求关联的产品 PRD（目前为飞书文档）：prd.md 是存档全文，点击链接直接打开飞书原文，右侧按钮一键复制链接。</p>
    <div className="react-actions">{doc.loading ? null : link ? <><a className="react-prd-link" href={link} target="_blank" rel="noreferrer">{link}</a><button type="button" className="react-copy-link-btn" title="复制 PRD 链接" onClick={copyLink}><Copy size={13} />{copied ? "已复制" : "复制链接"}</button></> : <><code className="react-doc-path">{req.prdPath}</code><a href={`/requirement-doc?id=${encodeURIComponent(req.id)}&doc=prd`}><Library size={15} />查看存档全文</a></>}</div>
    {doc.error ? <p className="react-effort-error">PRD 加载失败：{doc.error}</p> : null}
  </section>
}
