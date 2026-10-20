import { ArrowLeft, CheckCheck, ChevronLeft, FileX, GitMerge, ListChecks, RefreshCw, Save, Undo2 } from "lucide-react"
import { useCallback, useEffect, useMemo, useState } from "react"
import type { MergeConflictDetailPayload, MergeConflictFile, MergeConflictResolution, MergeConflictSegment } from "../types"
import { fetchJson, postJson } from "../lib/api"
import { EmptyCard, ErrorCard, LoadingCard, PageChrome } from "../components/ui"
import { RequirementsData } from "./projects"

// 与后端 split_lines 一致：按 \n 分行并保留行尾
function splitKeep(text: string): string[] {
  const parts = text.split("\n")
  const lines = parts.slice(0, -1).map((p) => p + "\n")
  const tail = parts[parts.length - 1]
  if (tail !== "") lines.push(tail)
  return lines
}

type ResolutionMap = Record<number, MergeConflictResolution>

// 组装解决后的完整文件内容；有未解决的冲突块时返回 null。
// 末段前的行统一补全换行，避免“上一行末尾无 \n + 下一段首行”粘连。
function composeContent(file: MergeConflictFile, res: ResolutionMap): string | null {
  const parts: string[] = []
  for (const seg of file.segments) {
    if (seg.type === "plain") {
      parts.push(...(seg.lines || []))
      continue
    }
    const r = res[seg.index as number]
    if (!r) return null
    if (r.type === "ours") parts.push(...(seg.ours || []))
    else if (r.type === "theirs") parts.push(...(seg.theirs || []))
    else parts.push(...r.lines)
  }
  return parts
    .map((l, i) => (i < parts.length - 1 && !l.endsWith("\n") ? l + "\n" : l))
    .join("")
}

function conflictsOf(file: MergeConflictFile): MergeConflictSegment[] {
  return file.segments.filter((s) => s.type === "conflict")
}

function SidePane({ title, subtitle, text, hunks, side, emphasize }: { title: string; subtitle: string; text: string; hunks: MergeConflictSegment[]; side: "ours" | "theirs"; emphasize: string }) {
  const lines = splitKeep(text)
  const ranges = hunks
    .map((h) => [h[side === "ours" ? "oursStart" : "theirsStart"] as number, h[side === "ours" ? "oursEnd" : "theirsEnd"] as number, h.index as number] as const)
    .filter(([s, e]) => s !== undefined && e !== undefined && e > s)
  const inRange = (i: number) => ranges.find(([s, e]) => i >= s && i < e)
  return (
    <div className={`react-cfl-pane react-cfl-pane-${side}`}>
      <div className="react-cfl-pane-head" style={{ color: emphasize }}>
        <strong>{title}</strong>
        <span>{subtitle}</span>
      </div>
      <div className="react-cfl-code">
        {lines.map((l, i) => {
          const hit = inRange(i)
          return (
            <div key={i} id={hit && i === hit[0] ? `cfl-${side}-${hit[2]}` : undefined} className={`react-cline${hit ? ` hit hit-${side}` : ""}`}>
              <span className="react-cline-ln">{i + 1}</span>
              <span className="react-cline-tx">{l.length ? l : "\n"}</span>
            </div>
          )
        })}
      </div>
    </div>
  )
}

export function RequirementMergeConflictPage() {
  const params = new URLSearchParams(window.location.search)
  const reqId = params.get("id") || params.get("reqId") || ""
  const repo = params.get("repo") || ""
  const targetParam = params.get("target") || ""
  const branchParam = params.get("branch") || ""
  const requirements = RequirementsData()
  const req = requirements.data?.requirements.find((r) => r.id === reqId)
  const [payload, setPayload] = useState<MergeConflictDetailPayload | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [selected, setSelected] = useState("")
  const [resolutions, setResolutions] = useState<Record<string, ResolutionMap>>({})
  const [toast, setToast] = useState<string | null>(null)
  const [doneMessage, setDoneMessage] = useState<string | null>(null)

  const query = useCallback(
    (extra: Record<string, string> = {}) => {
      const q = new URLSearchParams({ id: reqId, repo, ...(targetParam ? { target: targetParam } : {}), ...(branchParam ? { branch: branchParam } : {}), ...extra })
      return q.toString()
    },
    [reqId, repo, targetParam, branchParam],
  )

  const load = useCallback(async () => {
    if (!reqId || !repo) return
    setLoading(true)
    setError(null)
    try {
      const next = await fetchJson<MergeConflictDetailPayload>(`/api/requirement/merge-conflict?${query({ _: String(Date.now()) })}`)
      setPayload(next)
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    } finally {
      setLoading(false)
    }
  }, [reqId, repo, targetParam, branchParam, query])

  useEffect(() => { load() }, [load])

  const files = payload?.files || []
  // 选中文件失效时自动跳到下一个待处理文件
  useEffect(() => {
    if (!files.length) { setSelected(""); return }
    if (files.some((f) => f.path === selected)) return
    const next = files.find((f) => f.status === "partial" && f.conflictCount > 0) || files[0]
    setSelected(next.path)
  }, [files, selected])

  const file = files.find((f) => f.path === selected) || null
  const fileRes: ResolutionMap = (file && resolutions[file.path]) || {}
  const fileConflicts = file ? conflictsOf(file) : []
  const fileUnresolved = fileConflicts.filter((h) => !fileRes[h.index as number]).length
  const totalUnresolved = files.reduce((acc, f) => acc + conflictsOf(f).filter((h) => !(resolutions[f.path] || {})[h.index as number]).length, 0)
  const unresolvableCount = files.filter((f) => f.status !== "partial").length
  const partialFiles = files.filter((f) => f.status === "partial")
  const decisionFiles = files.filter((f) => f.status !== "partial")
  const canComplete = payload && files.length === 0 && !doneMessage
  const composed = file ? composeContent(file, fileRes) : null

  // 合并结果栏的行：plain 行带当前已选方案下的行号，冲突块独立渲染
  const resultRows = useMemo(() => {
    if (!file) return []
    let ln = 0
    const rows: Array<{ kind: "line"; ln: number; text: string } | { kind: "block"; hunk: MergeConflictSegment }> = []
    for (const seg of file.segments) {
      if (seg.type === "plain") {
        for (const l of seg.lines || []) {
          ln += 1
          rows.push({ kind: "line", ln, text: l })
        }
      } else {
        rows.push({ kind: "block", hunk: seg })
      }
    }
    return rows
  }, [file, resolutions])

  const choose = (index: number, type: "ours" | "theirs") => {
    if (!file) return
    setResolutions((prev) => ({ ...prev, [file.path]: { ...prev[file.path], [index]: { type } } }))
  }
  const chooseCustom = (index: number, initial: string[]) => {
    if (!file) return
    setResolutions((prev) => ({ ...prev, [file.path]: { ...prev[file.path], [index]: { type: "custom", lines: initial } } }))
  }
  const editCustom = (index: number, text: string) => {
    if (!file) return
    setResolutions((prev) => ({ ...prev, [file.path]: { ...prev[file.path], [index]: { type: "custom", lines: splitKeep(text) } } }))
  }
  const resetChoice = (index: number) => {
    if (!file) return
    const next = { ...(resolutions[file.path] || {}) }
    delete next[index]
    setResolutions((prev) => ({ ...prev, [file.path]: next }))
  }

  const flash = (message: string) => {
    setToast(message)
    window.setTimeout(() => setToast(null), 3200)
  }

  const saveFile = async () => {
    if (!file || composed === null) return
    setBusy(true)
    try {
      const r = await postJson<{ remainingFiles: number }>(`/api/requirement/merge-conflict/resolve-file`, { reqId, repo, target: targetParam || undefined, branch: branchParam || undefined, path: file.path, content: composed })
      flash(`已保存并暂存 ${file.path}；剩余 ${r.remainingFiles} 个冲突文件`)
      setResolutions((prev) => { const next = { ...prev }; delete next[file.path]; return next })
      await load()
    } catch (err) {
      flash(`保存失败：${err instanceof Error ? err.message : String(err)}`)
    } finally {
      setBusy(false)
    }
  }

  const decideFile = async (path: string, action: "keep-ours" | "keep-theirs" | "accept-delete", label: string) => {
    setBusy(true)
    try {
      await postJson(`/api/requirement/merge-conflict/resolve-file`, { reqId, repo, target: targetParam || undefined, branch: branchParam || undefined, path, action })
      flash(`${path}：${label}`)
      await load()
    } catch (err) {
      flash(`操作失败：${err instanceof Error ? err.message : String(err)}`)
    } finally {
      setBusy(false)
    }
  }

  const completeMerge = async () => {
    setBusy(true)
    try {
      const r = await postJson<{ message: string }>(`/api/requirement/merge-conflict/complete`, { reqId, repo, target: targetParam || undefined, branch: branchParam || undefined })
      setDoneMessage(r.message)
    } catch (err) {
      flash(`完成合并失败：${err instanceof Error ? err.message : String(err)}`)
    } finally {
      setBusy(false)
    }
  }

  const abortMerge = async () => {
    if (!window.confirm("确定放弃本次合并？该仓库的 merge worktree 将被清理，所有解决进度作废。")) return
    setBusy(true)
    try {
      await postJson(`/api/requirement/merge-conflict/abort`, { reqId, repo, target: targetParam || undefined, branch: branchParam || undefined })
      window.location.href = `/requirement-merge?id=${encodeURIComponent(reqId)}`
    } catch (err) {
      flash(`放弃合并失败：${err instanceof Error ? err.message : String(err)}`)
      setBusy(false)
    }
  }

  const jumpTo = (side: "ours" | "theirs", index: number) => {
    const el = document.getElementById(`cfl-${side}-${index}`)
    el?.scrollIntoView({ block: "center", behavior: "smooth" })
  }

  const title = req?.title || reqId || "合并冲突"
  const summary = payload?.summary
  const backHref = `/requirement?id=${encodeURIComponent(reqId)}`

  return <PageChrome
    icon={<GitMerge size={15} />}
    eyebrow="Merge Conflicts"
    title={title}
    description={payload ? `${payload.repoName}：${payload.sourceBranch} → ${payload.targetBranch}（${payload.target}）· 冲突 worktree 内直接解决，无需打开 IDE` : "解决合并冲突"}
    actions={<>
      <a href={backHref}><ArrowLeft size={15} />返回需求</a>
      <button onClick={() => load()} disabled={loading || busy}><RefreshCw size={15} className={loading ? "react-spin" : ""} />刷新</button>
      {canComplete
        ? <button className="react-cfl-complete" onClick={completeMerge} disabled={busy}><CheckCheck size={15} />完成合并并推送</button>
        : null}
      <button className="react-cfl-abort" onClick={abortMerge} disabled={busy || doneMessage !== null}><FileX size={15} />放弃合并</button>
    </>}
  >
    {doneMessage ? (
      <section className="react-panel react-cfl-done-panel">
        <div className="react-cfl-done"><CheckCheck size={22} /><strong>合并完成</strong><p>{doneMessage}</p><span>worktree 已清理；可返回需求页继续后续流程。</span></div>
        <div className="react-actions"><a href={`/requirement-merge?id=${encodeURIComponent(reqId)}`}><ChevronLeft size={15} />查看合并状态</a><a href={backHref}><ArrowLeft size={15} />返回需求</a></div>
      </section>
    ) : loading ? <LoadingCard label="正在计算冲突详情（细粒度三方合并）…" />
    : error ? <ErrorCard error={error} />
    : !payload || files.length === 0 ? (
      <section className="react-panel">
        <EmptyCard>
          当前没有未解决的冲突文件。{canComplete ? "冲突已全部解决，可点击右上角「完成合并并推送」。" : "若合并未完成，可回需求页重新触发合并。"}
        </EmptyCard>
      </section>
    ) : (
      <section className="react-panel react-cfl-panel">
        <div className="react-cfl-toolbar">
          <span className="react-cfl-stat"><ListChecks size={14} />待处理文件 {files.length} · 真冲突 {totalUnresolved} 处 · 需决策 {unresolvableCount} 文件</span>
          {summary && summary.autoHunks ? <span className="react-cfl-stat auto">已自动解决 {summary.autoHunks} 处差异{summary.autoResolvedFiles ? `（${summary.autoResolvedFiles} 文件）` : ""}</span> : null}
          <code className="react-cfl-worktree">{payload.worktreePath}</code>
        </div>
        <div className="react-cfl-layout">
          {/* 左侧：冲突文件列表 */}
          <aside className="react-cfl-files">
            <div className="react-cfl-group">
              <h4>冲突文件（{partialFiles.length}）</h4>
              {partialFiles.map((f) => {
                const res = resolutions[f.path] || {}
                const remaining = conflictsOf(f).filter((h) => !res[h.index as number]).length
                return <button key={f.path} className={`react-cfl-file${f.path === selected ? " active" : ""}${remaining === 0 ? " ready" : ""}`} onClick={() => setSelected(f.path)}>
                  <span className="react-cfl-file-icon">{remaining === 0 ? "✔" : "●"}</span>
                  <span className="react-cfl-file-path" title={f.path}>{f.path}</span>
                  <em>{remaining === 0 ? "可保存" : `${remaining} 处`}</em>
                </button>
              })}
              {partialFiles.length === 0 ? <p className="react-cfl-none">无</p> : null}
            </div>
            {decisionFiles.length ? (
              <div className="react-cfl-group">
                <h4>需人工决策（{decisionFiles.length}）</h4>
                {decisionFiles.map((f) => (
                  <button key={f.path} className={`react-cfl-file decision${f.path === selected ? " active" : ""}`} onClick={() => setSelected(f.path)}>
                    <span className="react-cfl-file-icon warn">▲</span>
                    <span className="react-cfl-file-path" title={f.path}>{f.path}</span>
                    <em>{f.status === "binary" ? "二进制" : "改动/删除"}</em>
                  </button>
                ))}
              </div>
            ) : null}
          </aside>

          {/* 右侧：三栏合并编辑器 */}
          <div className="react-cfl-editor">
            {!file ? <EmptyCard>选择左侧文件开始处理。</EmptyCard> : file.status !== "partial" ? (
              <div className="react-cfl-decision">
                <h3><FileX size={16} />{file.path}</h3>
                <p className="react-cfl-reason">{file.reason || "该文件无法按行自动合并，需要人工决策保留哪一侧。"}</p>
                <div className="react-actions">
                  {file.hasOurs ? <button onClick={() => decideFile(file.path, "keep-ours", "已保留目标分支版本")} disabled={busy}>保留目标分支版本（ours）</button> : null}
                  {file.hasTheirs ? <button onClick={() => decideFile(file.path, "keep-theirs", "已保留需求分支版本")} disabled={busy}>保留需求分支版本（theirs）</button> : null}
                  <button className="react-cfl-abort" onClick={() => decideFile(file.path, "accept-delete", "已接受删除")} disabled={busy}>接受删除（git rm）</button>
                </div>
                {file.hasOurs ? <details className="react-cfl-blob"><summary>目标分支（ours）内容</summary><pre>{file.oursText}</pre></details> : null}
                {file.hasTheirs ? <details className="react-cfl-blob"><summary>需求分支（theirs）内容</summary><pre>{file.theirsText}</pre></details> : null}
              </div>
            ) : (
              <>
                <div className="react-cfl-filebar">
                  <strong title={file.path}>{file.path}</strong>
                  <span>{fileConflicts.length} 处冲突 · 已处理 {fileConflicts.length - fileUnresolved}</span>
                  <button onClick={() => saveFile()} disabled={busy || composed === null || !fileConflicts.length}>
                    <Save size={14} />{composed === null ? `还有 ${fileUnresolved} 处未解决` : "保存并暂存"}
                  </button>
                </div>
                {file.autoHunks ? <p className="react-cfl-hint">本文件已自动合并 {file.autoHunks} 处无歧义差异（下方仅剩真冲突需决策）；左侧=目标分支 {payload?.targetBranch}，右侧=需求分支 {payload?.sourceBranch}。</p> : null}
                <div className="react-cfl-panes">
                  <SidePane
                    title={`目标分支（ours）`}
                    subtitle={payload?.targetBranch || ""}
                    text={file.oursText}
                    hunks={fileConflicts}
                    side="ours"
                    emphasize="var(--react-cyan)"
                  />
                  <div className="react-cfl-pane react-cfl-pane-result">
                    <div className="react-cfl-pane-head result"><strong>合并结果</strong><span>点击冲突块操作；支持手动融合</span></div>
                    <div className="react-cfl-code">
                      {resultRows.map((row, i) => {
                        if (row.kind === "line") {
                          return (
                            <div key={`p-${i}`} className="react-cline">
                              <span className="react-cline-ln">{row.ln}</span>
                              <span className="react-cline-tx">{row.text.length ? row.text : "\n"}</span>
                            </div>
                          )
                        }
                        const h = row.hunk
                        const r = fileRes[h.index as number]
                        return (
                          <div key={`c-${i}`} className={`react-cfl-block${r ? " resolved" : ""}`}>
                            <div className="react-cfl-block-bar" onClick={() => { jumpTo("ours", h.index as number); jumpTo("theirs", h.index as number) }}>
                              <span className="react-cfl-block-tag">冲突 {(h.index as number) + 1} · {h.kind === "insert-race" ? "插入竞争" : "内容冲突"}</span>
                              <div className="react-cfl-block-btns" onClick={(e) => e.stopPropagation()}>
                                <button className="pick ours" onClick={() => choose(h.index as number, "ours")}>◀ 使用目标分支</button>
                                <button className="pick theirs" onClick={() => choose(h.index as number, "theirs")}>使用需求分支 ▶</button>
                                <button className="pick custom" onClick={() => chooseCustom(h.index as number, h.ours || [])}>手动编辑</button>
                                {r ? <button className="pick undo" onClick={() => resetChoice(h.index as number)}><Undo2 size={12} />撤销</button> : null}
                              </div>
                            </div>
                            {!r ? (
                              <div className="react-cfl-block-sides">
                                <pre className="side ours">{(h.ours || []).join("")}</pre>
                                <pre className="side theirs">{(h.theirs || []).join("")}</pre>
                              </div>
                            ) : r.type === "custom" ? (
                              <textarea
                                className="react-cfl-custom"
                                value={r.lines.join("")}
                                rows={Math.min(18, Math.max(3, r.lines.length))}
                                spellCheck={false}
                                onChange={(e) => editCustom(h.index as number, e.target.value)}
                              />
                            ) : (
                              <pre className={`side chosen ${r.type}`}>{(r.type === "ours" ? h.ours || [] : h.theirs || []).join("")}</pre>
                            )}
                          </div>
                        )
                      })}
                    </div>
                  </div>
                  <SidePane
                    title="需求分支（theirs）"
                    subtitle={payload?.sourceBranch || ""}
                    text={file.theirsText}
                    hunks={fileConflicts}
                    side="theirs"
                    emphasize="#86efac"
                  />
                </div>
              </>
            )}
          </div>
        </div>
        {toast ? <div className="react-cfl-toast">{toast}</div> : null}
      </section>
    )}
  </PageChrome>
}
