import { ArrowLeft, Library } from "lucide-react"
import type { RequirementDocPayload } from "../types"
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

export function RequirementDocPage() {
  const params = new URLSearchParams(window.location.search)
  const id = params.get("id") || params.get("reqId") || ""
  const docType = params.get("doc") || params.get("file") || "background"
  const doc = useFetch<RequirementDocPayload>(id ? `/api/requirement/doc?id=${encodeURIComponent(id)}&file=${encodeURIComponent(docType)}` : null, [id, docType])
  const title = DOC_TITLES[docType] ?? docType
  return <PageChrome icon={<Library size={15} />} eyebrow="Requirement Doc" title={title} description={id ? `需求 ${id}` : undefined} actions={<><a href={id ? `/requirement?id=${encodeURIComponent(id)}` : "/projects"}><ArrowLeft size={15} />返回需求</a></>}>
    <section className="react-panel react-doc-page-panel">
      <PanelHead kicker="Document" title={title} chip={doc.loading ? "loading" : doc.data?.file || docType} />
      {doc.error ? <ErrorCard error={doc.error} /> : doc.loading ? <LoadingCard label="正在加载文档…" /> : !doc.data?.exists ? (
        <EmptyCard>暂无内容。可在需求澄清阶段生成或更新该文档。</EmptyCard>
      ) : <Markdown text={doc.data.content || ""} />}
    </section>
  </PageChrome>
}
