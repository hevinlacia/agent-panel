import { ArrowLeft, Library } from "lucide-react"
import type { DocPartsPayload, RequirementDocPayload } from "../types"
import { useFetch } from "../lib/api"
import { Markdown } from "../markdown"
import { EmptyCard, ErrorCard, LoadingCard, PageChrome, PanelHead } from "../components/ui"

const DOC_TITLES: Record<string, string> = {
  "background": "业务背景文档",
  "technical-plan": "技术方案",
  "release-manifest": "上线清单",
  "experience-summary": "经验总结闭环",
  "troubleshooting": "排查经验",
  "incident": "问题档案",
  "root-cause": "根因与修复决策",
  "review": "代码审查",
  "release-check": "发布预检",
  "test": "测试用例",
  "test-scenario": "测试场景文档",
  "notes": "执行笔记",
  "meta": "需求信息",
}

function humanBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`
}

/** 需求文档全文页：file 参数支持主文档 docType 与分册相对路径（docs/<doc>/<NNN>-<slug>.md）。
 *  主文档视图附带分册清单（主文档索引化后的入口）；分册视图带「返回主文件」。 */
export function RequirementDocPage() {
  const params = new URLSearchParams(window.location.search)
  const id = params.get("id") || params.get("reqId") || ""
  const docType = params.get("doc") || params.get("file") || "background"
  const isPart = docType.includes("/")
  const base = isPart ? docType.split("/")[1] ?? "" : docType
  const doc = useFetch<RequirementDocPayload>(id ? `/api/requirement/doc?id=${encodeURIComponent(id)}&file=${encodeURIComponent(docType)}` : null, [id, docType])
  const parts = useFetch<DocPartsPayload>(id && !isPart ? `/api/requirement/doc-parts?reqId=${encodeURIComponent(id)}&file=${encodeURIComponent(docType)}` : null, [id, docType, isPart])
  const title = isPart
    ? docType.split("/").pop() || docType
    : DOC_TITLES[docType] ?? docType
  return <PageChrome icon={<Library size={15} />} eyebrow="Requirement Doc" title={title} description={id ? `需求 ${id}` : undefined} actions={<>{isPart && base ? <a href={`/requirement-doc?id=${encodeURIComponent(id)}&doc=${encodeURIComponent(base)}`}><ArrowLeft size={15} />返回主文件</a> : null}<a href={id ? `/requirement?id=${encodeURIComponent(id)}` : "/projects"}><ArrowLeft size={15} />返回需求</a></>}>
    <section className="react-panel react-doc-page-panel">
      <PanelHead kicker={isPart ? "Document Part" : "Document"} title={title} chip={doc.loading ? "loading" : doc.data?.exists ? (doc.data.file || docType) : docType} />
      {isPart ? <p className="react-muted">分册路径 <code>{docType}</code>；主文件是索引，本页展示分册全文。</p> : null}
      {doc.error ? <ErrorCard error={doc.error} /> : doc.loading ? <LoadingCard label="正在加载文档…" /> : !doc.data?.exists ? (
        <EmptyCard>暂无内容。可在需求澄清阶段生成或更新该文档。</EmptyCard>
      ) : <Markdown text={doc.data.content || ""} />}
      {!isPart && parts.data?.mainDoc?.overSplitThreshold ? <p className="react-effort-error">主文档 {humanBytes(parts.data.mainDoc.bytes)} 超过拆分阈值 {humanBytes(parts.data.splitWarnBytes)}：建议把明细用 POST /api/requirement/doc-part 拆到分册（单分册建议 ≤300 行），主文件只留概览与索引。</p> : null}
      {!isPart && (parts.data?.count ?? 0) > 0 ? <div className="react-doc-parts"><PanelHead kicker="Doc Parts" title="分册" chip={String(parts.data?.count)} /><p className="react-muted">主文件是索引，明细在以下分册中（按序号排序）；点击查看分册全文，agent 也可按相对路径直接读取。</p><div className="react-card-meta">{parts.data?.parts.map((p) => <span key={p.relPath} className="react-linked-issue-chip"><a href={`/requirement-doc?id=${encodeURIComponent(id)}&doc=${encodeURIComponent(p.relPath)}`}>{p.filename}</a>{p.title ? <em className="react-muted">{p.title}</em> : null}<span className="react-muted">{humanBytes(p.bytes)}</span>{p.indexLinked === false ? <em className="react-effort-error" title="主文档索引段缺少该分册链接">未索引</em> : null}</span>)}</div></div> : null}
    </section>
  </PageChrome>
}
