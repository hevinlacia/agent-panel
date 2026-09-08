/**
 * Role: right-hand inspector of the requirement diff page. Shows the
 * annotation (summary / key variables / flow diagram / hunk notes) for the
 * file the user is reading, highlights notes for the clicked hunk, and lets
 * the user hand-edit the annotation JSON. Code display itself stays
 * untouched — all explanation lives here.
 */
import { AlignLeft, Boxes, Braces, MessageSquareText, Save, ScrollText, Table2, TriangleAlert, X } from "lucide-react"
import { useEffect, useState } from "react"
import { Markdown } from "../../markdown"
import { EmptyCard } from "../../components/ui"
import type { AnnotationNote, CodeFileAnnotation } from "../../types"
import { MermaidFlow } from "./mermaid-flow"
import type { MatchedNote } from "../../lib/annotations"

interface AnnotationPanelProps {
  /** repo/path of the annotated file, e.g. wms-web/src/pages/x.tsx */
  fileLabel: string
  annotation: CodeFileAnnotation | null
  stale: boolean
  activeNotes: MatchedNote[]
  unmatchedNotes: AnnotationNote[]
  onLocate: (lineIndex: number) => void
  onSave: (next: CodeFileAnnotation) => Promise<void>
  saving: boolean
}

function SectionHead({ icon, title }: { icon: React.ReactNode; title: string }) {
  return <div className="react-annotation-section-head">{icon}<span>{title}</span></div>
}

export function AnnotationPanel({ fileLabel, annotation, stale, activeNotes, unmatchedNotes, onLocate, onSave, saving }: AnnotationPanelProps) {
  const [editing, setEditing] = useState(false)
  const [draft, setDraft] = useState("")
  const [editError, setEditError] = useState<string | null>(null)

  useEffect(() => { setEditing(false); setEditError(null) }, [fileLabel])

  const startEdit = () => {
    setEditError(null)
    setDraft(JSON.stringify(annotation ?? { repo: "", path: "", summary: "", variables: [], flow: "", notes: [] }, null, 2))
    setEditing(true)
  }
  const submitEdit = async () => {
    let parsed: CodeFileAnnotation
    try {
      parsed = JSON.parse(draft) as CodeFileAnnotation
    } catch (err) {
      setEditError(`JSON 解析失败：${err instanceof Error ? err.message : String(err)}`)
      return
    }
    setEditError(null)
    await onSave(parsed)
    setEditing(false)
  }

  const hasContent = Boolean(
    annotation?.summary?.trim() || annotation?.variables?.length || annotation?.flow?.trim() || annotation?.notes?.length,
  )

  return <aside className="react-diff-inspector">
    <div className="react-annotation-head">
      <div className="react-annotation-title"><ScrollText size={14} /><strong>说明</strong><em title={fileLabel}>{fileLabel.split("/").pop() || fileLabel}</em></div>
      {!editing ? <button type="button" className="react-annotation-edit-btn" onClick={startEdit} title="编辑该文件备注 JSON"><Braces size={13} />编辑</button> : null}
    </div>
    {stale ? <div className="react-annotation-stale"><TriangleAlert size={13} />备注基于旧 diff，锚定可能过期</div> : null}
    {editing ? <div className="react-annotation-editor">
      <textarea value={draft} onChange={(e) => setDraft(e.target.value)} spellCheck={false} />
      {editError ? <p className="react-annotation-edit-error">{editError}</p> : null}
      <div className="react-annotation-editor-actions">
        <button type="button" onClick={submitEdit} disabled={saving}><Save size={13} />{saving ? "保存中…" : "保存"}</button>
        <button type="button" onClick={() => setEditing(false)} disabled={saving}><X size={13} />取消</button>
      </div>
    </div> : null}
    {!editing && !hasContent ? <EmptyCard>
      <p>该文件暂无说明。</p>
      <p>在 pi 会话中对需求运行 <code>diff-annotate</code> skill 生成讲解，或点击右上角「编辑」手写。</p>
    </EmptyCard> : null}
    {!editing && annotation ? <div className="react-annotation-body">
      {activeNotes.length ? <section className="react-annotation-section react-annotation-active">
        <SectionHead icon={<MessageSquareText size={13} />} title="当前段落备注" />
        {activeNotes.map(({ note }, i) => <div key={i} className="react-annotation-note react-md-sm"><Markdown text={note.note} /></div>)}
      </section> : null}
      {annotation.summary?.trim() ? <section className="react-annotation-section">
        <SectionHead icon={<AlignLeft size={13} />} title="改动摘要" />
        <div className="react-md-sm"><Markdown text={annotation.summary} /></div>
      </section> : null}
      {annotation.variables?.length ? <section className="react-annotation-section">
        <SectionHead icon={<Table2 size={13} />} title="关键变量" />
        <table className="react-annotation-vars"><thead><tr><th>变量</th><th>含义</th></tr></thead><tbody>
          {annotation.variables.map((v, i) => <tr key={i}>
            <td><code>{v.name}</code>{v.kind ? <em className="react-annotation-var-kind">{v.kind}</em> : null}</td>
            <td>{v.meaning || "-"}{v.why ? <small>{v.why}</small> : null}</td>
          </tr>)}
        </tbody></table>
      </section> : null}
      {annotation.flow?.trim() ? <section className="react-annotation-section">
        <SectionHead icon={<Boxes size={13} />} title={annotation.flowTitle?.trim() || "数据 / 状态流转"} />
        <MermaidFlow code={annotation.flow} />
      </section> : null}
      {annotation.notes?.length ? <section className="react-annotation-section">
        <SectionHead icon={<MessageSquareText size={13} />} title={`段落备注（${annotation.notes.length}）`} />
        <ul className="react-annotation-note-list">
          {annotation.notes.map((note, i) => {
            const anchored = note.anchor?.hunkHeader
            return <li key={i}>
              <div className="react-annotation-note react-md-sm"><Markdown text={note.note} /></div>
              {anchored ? <button type="button" className="react-annotation-note-loc" onClick={() => onLocate(i)}><code>{anchored}</code></button> : null}
            </li>
          })}
        </ul>
        {unmatchedNotes.length ? <p className="react-annotation-unmatched-hint">{unmatchedNotes.length} 条备注未能定位到当前 diff 的代码块，已按序号列出。</p> : null}
      </section> : null}
    </div> : null}
  </aside>
}
