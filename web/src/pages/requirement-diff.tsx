import { ArrowLeft, FileCode2, GitBranch, MessageSquareText, Plus, Redo2, RefreshCw, Search, Undo2 } from "lucide-react"
import { useEffect, useMemo, useRef, useState } from "react"
import type { AnnotationsPayload, BranchRoundsPayload, CodeAnnotations, CodeDiffSnapshot, CodeFileAnnotation, DiffSnapshotsPayload, MasterDiffPayload } from "../types"
import { fetchJson, postForm, postJson, putJson, useFetch } from "../lib/api"
import { compactPath, diffDomId, diffLineDomId, parseUnifiedDiffFiles, reviewStats, shortFileName } from "../lib/diff"
import { formatDateTime } from "../lib/format"
import { annotationStaleRepos, buildAnnotationIndex, hunkOwners, matchHunkNotes } from "../lib/annotations"
import { AnnotationPanel } from "../features/requirements/annotation-panel"
import { EmptyCard, ErrorCard, LoadingCard, PageChrome } from "../components/ui"
import { RequirementsData } from "./projects"

export function RequirementDiffPage() {
  const params = new URLSearchParams(window.location.search)
  const reqId = params.get("id") || params.get("reqId") || ""
  const initialBase = params.get("base") || "origin/master"
  // 分支登记轮次：1 = 原始 branches.json；>=2 = 合入生产后的修复轮次 branches-round-<n>.json。
  const initialRound = Math.max(1, Number(params.get("round")) || 1)
  const requirements = RequirementsData()
  const req = requirements.data?.requirements.find((r) => r.id === reqId)
  const [baseRef, setBaseRef] = useState(initialBase)
  const [round, setRound] = useState(initialRound)
  const [creatingRound, setCreatingRound] = useState(false)
  const rounds = useFetch<BranchRoundsPayload>(reqId ? `/api/requirement/branch-rounds?reqId=${encodeURIComponent(reqId)}` : null, [reqId])
  const [loadingDiff, setLoadingDiff] = useState(false)
  const [error, setError] = useState<string | null>(null)
  /** 快照对比模式：差异栈（新在前）+ 当前查看的下标；进入页面只读栈，点刷新才生成并入栈。 */
  const [snapshots, setSnapshots] = useState<CodeDiffSnapshot[]>([])
  const [viewIndex, setViewIndex] = useState(0)
  const review = snapshots[viewIndex] || null
  const files = useMemo(() => parseUnifiedDiffFiles(review), [review])
  const stats = reviewStats(review)
  const [activeKey, setActiveKey] = useState("")
  /** 行级联动状态：用户点击的 diff 行（key + 行下标），驱动右侧说明面板与 hunk 高亮。 */
  const [activeLine, setActiveLine] = useState<{ key: string; line: number } | null>(null)
  const [savingAnnotation, setSavingAnnotation] = useState(false)
  /** 顶部统一横向滚动条：diff 内容（所有文件卡片）放进同一个横向滚动容器，顶部 sticky 工具栏下方挂一条滚动条统一控制，替代每行超长代码各自的滚动条。 */
  const scrollBodyRef = useRef<HTMLDivElement | null>(null)
  const scrollBarRef = useRef<HTMLDivElement | null>(null)
  const [hScrollWidth, setHScrollWidth] = useState(0)
  useEffect(() => {
    const body = scrollBodyRef.current
    if (!body) return
    const measure = () => {
      const bar = scrollBarRef.current
      // 补偿顶部滚动条与内容区 clientWidth 的差值（边框等），保证两侧滚动范围一致、scrollLeft 可 1:1 同步。
      setHScrollWidth(body.scrollWidth + Math.max(0, (bar?.clientWidth ?? 0) - body.clientWidth))
    }
    measure()
    const ro = new ResizeObserver(measure)
    ro.observe(body)
    for (const child of Array.from(body.children)) ro.observe(child)
    window.addEventListener("resize", measure)
    document.fonts?.ready.then(measure).catch(() => {})
    return () => { ro.disconnect(); window.removeEventListener("resize", measure) }
  }, [files, loadingDiff])
  // 双向同步顶部滚动条与内容区；赋值触发的对侧 scroll 事件里两侧值已相等，不会来回抖动。
  const onHScrollBarScroll = () => {
    const body = scrollBodyRef.current
    const bar = scrollBarRef.current
    if (body && bar && body.scrollLeft !== bar.scrollLeft) body.scrollLeft = bar.scrollLeft
  }
  const onHScrollBodyScroll = () => {
    const body = scrollBodyRef.current
    const bar = scrollBarRef.current
    if (body && bar && bar.scrollLeft !== body.scrollLeft) bar.scrollLeft = body.scrollLeft
  }
  const annotations = useFetch<AnnotationsPayload>(reqId ? `/api/requirement/annotations?reqId=${encodeURIComponent(reqId)}` : null, [reqId])
  // 进入页面只加载当前轮次已保存的快照栈，不自动生成；生成由「刷新代码差异」显式触发。
  useEffect(() => {
    if (!reqId) return
    let cancelled = false
    fetchJson<DiffSnapshotsPayload>(`/api/requirement/diff-snapshots?reqId=${encodeURIComponent(reqId)}&round=${round}`)
      .then((payload) => { if (!cancelled) { setSnapshots(payload.snapshots || []); setViewIndex(0) } })
      .catch(() => { if (!cancelled) setSnapshots([]) })
    return () => { cancelled = true }
  }, [reqId, round])
  // 生成新差异并入栈（后端按轮次保留最近 5 版）；base 不变时点刷新也会重新生成，旧版本保留可回退。
  const generateDiff = async (base = baseRef) => {
    if (!reqId) return
    setLoadingDiff(true)
    setError(null)
    try {
      const payload = await postForm<MasterDiffPayload>("/api/requirement/master-diff", { reqId, baseRef: base, round: String(round) })
      setSnapshots(payload.snapshots || [])
      setViewIndex(0)
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    } finally {
      setLoadingDiff(false)
    }
  }
  useEffect(() => { setActiveLine(null) }, [viewIndex])
  useEffect(() => {
    if (!files.length) { setActiveKey(""); return }
    const exists = files.some((item) => `${item.repo.repoName}:${item.file.path}` === activeKey)
    if (!exists) setActiveKey(`${files[0].repo.repoName}:${files[0].file.path}`)
  }, [files, activeKey])
  const activeIndex = Math.max(0, files.findIndex((item) => `${item.repo.repoName}:${item.file.path}` === activeKey))
  const activeView = files.find((item) => `${item.repo.repoName}:${item.file.path}` === activeKey) || null
  const annotationIndex = useMemo(() => buildAnnotationIndex(files, annotations.data?.annotations), [files, annotations.data])
  const staleRepos = useMemo(() => annotationStaleRepos(annotations.data?.annotations, files), [annotations.data, files])
  const activeAnnotation = annotationIndex.get(activeKey) || null
  const activeHunkOwners = useMemo(() => hunkOwners(activeView?.lines || []), [activeView])
  const hunkNotes = useMemo(() => matchHunkNotes(activeView?.lines || [], activeAnnotation?.notes), [activeView, activeAnnotation])
  const activeHunkLine = activeLine && activeLine.key === activeKey ? activeHunkOwners[activeLine.line] ?? -1 : -1
  const noteLocateMap = useMemo(() => {
    const map = new Map<number, number>()
    const notes = activeAnnotation?.notes || []
    for (const hit of hunkNotes.matched) {
      const idx = notes.indexOf(hit.note)
      if (idx >= 0) map.set(idx, hit.lineIndex)
    }
    return map
  }, [hunkNotes, activeAnnotation])
  const scrollToFile = (key: string) => {
    setActiveKey(key)
    setActiveLine(null)
    document.getElementById(diffDomId(key))?.scrollIntoView({ behavior: "smooth", block: "start" })
  }
  const locateNote = (noteIndex: number) => {
    const lineIndex = noteLocateMap.get(noteIndex)
    if (lineIndex == null) return
    setActiveLine({ key: activeKey, line: lineIndex })
    document.getElementById(diffLineDomId(activeKey, lineIndex))?.scrollIntoView({ behavior: "smooth", block: "center" })
  }
  const saveAnnotation = async (next: CodeFileAnnotation) => {
    if (!reqId) return
    setSavingAnnotation(true)
    try {
      const current: CodeAnnotations = { ...(annotations.data?.annotations || {}) }
      const list = [...(current.files || [])]
      const idx = list.findIndex((f) => f.repo === next.repo && f.path === next.path)
      if (idx >= 0) list[idx] = next
      else list.push(next)
      current.files = list
      await putJson("/api/requirement/annotations", { reqId, annotations: current })
      annotations.refresh()
    } finally {
      setSavingAnnotation(false)
    }
  }
  const changeBase = (next: string) => {
    setBaseRef(next)
    const q = new URLSearchParams(window.location.search)
    q.set("id", reqId)
    q.set("base", next)
    window.history.replaceState(null, "", `/requirement-diff?${q.toString()}`)
    // 切基准分支即以新基线生成并保存一版新快照，旧快照仍在栈内可回退。
    generateDiff(next)
  }
  // 切换分支登记轮次：只换轮次不自动生成，加载该轮次已保存的快照栈；URL 持久化以便刷新后留在同一轮次。
  const changeRound = (next: number) => {
    const n = Math.max(1, next)
    setRound(n)
    const q = new URLSearchParams(window.location.search)
    q.set("id", reqId)
    q.set("round", String(n))
    window.history.replaceState(null, "", `/requirement-diff?${q.toString()}`)
  }
  // 新建修复轮次：后端拷贝最新轮次 repo 结构并清空分支列表（branches-round-<n+1>.json），原轮次封版；创建后切到新轮次。
  const createRound = async () => {
    if (!reqId || creatingRound) return
    setCreatingRound(true)
    try {
      const res = await postJson<{ ok: boolean; round: number }>("/api/requirement/branch-rounds", { reqId })
      rounds.refresh()
      changeRound(res.round)
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    } finally {
      setCreatingRound(false)
    }
  }
  const title = req?.title || reqId || "分支差异"
  return <PageChrome icon={<GitBranch size={15} />} eyebrow="Diff" title={title} description="快照对比：生成的代码差异保存为本地快照，点刷新生成新版（旧版保留可回退）；分支登记支持多轮次，原始分支合入生产后新建修复轮次登记后续改动。右侧查看/编辑文件说明。" actions={<><a href={`/requirement?id=${encodeURIComponent(reqId)}`}><ArrowLeft size={15} />返回需求</a><button onClick={() => generateDiff()} disabled={loadingDiff || !reqId}><RefreshCw size={15} className={loadingDiff ? "react-spin" : ""} />{loadingDiff ? "生成中…" : snapshots.length ? "刷新代码差异" : "生成代码差异"}</button>{viewIndex < snapshots.length - 1 ? <button onClick={() => setViewIndex((i) => Math.min(i + 1, snapshots.length - 1))} disabled={loadingDiff}><Undo2 size={15} />回退上一版</button> : null}{viewIndex > 0 ? <button onClick={() => setViewIndex(0)} disabled={loadingDiff}><Redo2 size={15} />回到最新</button> : null}</>}>
    <section className="react-diff-shell">
      <aside className="react-diff-sidebar"><div className="react-diff-round"><span>Round</span><select value={round} onChange={(e) => changeRound(Number(e.target.value))}>{(rounds.data?.rounds || []).some((r) => r.round === round) ? null : <option key={round} value={round}>轮次 {round}</option>}{(rounds.data?.rounds || []).map((r) => <option key={r.round} value={r.round}>轮次 {r.round}{r.round === 1 ? "（原始）" : "（修复）"}{r.sealed ? " · 已封版" : ""}</option>)}</select><button className="react-diff-round-add" onClick={createRound} disabled={creatingRound || !reqId}><Plus size={13} />{creatingRound ? "创建中…" : "新建修复轮次"}</button>{rounds.data && rounds.data.latest > 1 && round === 1 ? <em>轮次 1 已封版：原需求分支已合入生产，修复改动登记在更新轮次。</em> : null}{rounds.data && rounds.data.latest === 0 ? <em>尚未登记 branches.json（req-branches-update）。</em> : null}</div><div className="react-diff-compare"><span>Compare</span><select value={baseRef} onChange={(e) => changeBase(e.target.value)}><option value="origin/master">origin/master</option><option value="origin/production">origin/production</option><option value="master">master</option><option value="production">production</option></select><em>and latest version</em></div><label className="react-diff-search"><Search size={14} /><input placeholder="Search files (Ctrl+P)" onChange={(e) => { const hit = files.find((f) => f.file.path.toLowerCase().includes(e.target.value.toLowerCase())); if (hit && e.target.value) scrollToFile(`${hit.repo.repoName}:${hit.file.path}`) }} /></label><div className="react-diff-file-list">{[...new Set(review?.repos?.map((r) => r.repoName) || [])].map((repoName) => {
          const repo = review?.repos?.find((r) => r.repoName === repoName)
          const repoFiles = files.filter((f) => f.repo.repoName === repoName)
          return <details key={repoName} className="react-diff-repo-group" open>
            <summary><strong>{repoName}</strong><em>base: {repo?.baseRef || baseRef}</em></summary>
            {repoFiles.length ? repoFiles.map((item) => { const key = `${item.repo.repoName}:${item.file.path}`; return <button key={key} className={key === activeKey ? "active" : ""} onClick={() => scrollToFile(key)}><FileCode2 size={14} /><span><strong>{shortFileName(item.file.path)}</strong><small>{compactPath(item.file.path)}</small></span><em>{annotationIndex.has(key) ? <MessageSquareText size={12} className="react-diff-annotated" /> : null}<b>+{item.file.additions}</b> <i>-{item.file.deletions}</i></em></button> }) : <p className="react-muted" style={{padding: '8px'}}>暂无文件差异</p>}
          </details>
        })}</div></aside>
      <main className="react-diff-main"><div className="react-diff-topbar"><div className="react-diff-toolbar"><div><strong>{stats.fileCount} files</strong><span className="react-review-add">+{stats.additions}</span><span className="react-review-del">-{stats.deletions}</span>{snapshots.length ? <span>快照 {viewIndex + 1}/{snapshots.length}</span> : null}{review?.savedAt ? <span>生成 {formatDateTime(review.savedAt)}</span> : review?.updatedAt ? <span>生成 {formatDateTime(review.updatedAt)}</span> : null}{viewIndex > 0 ? <span className="react-warn-note">⚠ 正在查看历史快照（base: {review?.baseRef || baseRef}），备注锚定以当前显示快照为准</span> : null}{review?.repos?.some((r) => r.diffTruncated) ? <span className="react-warn-note">⚠ 部分仓库 diff 超过输出上限被截断，缺失内容可分仓或减小差异后重试</span> : null}{staleRepos.length ? <span className="react-warn-note">⚠ {staleRepos.join(" / ")} 的备注基于旧 diff</span> : null}</div><span>{activeIndex + 1}/{Math.max(files.length, 1)}</span></div>{files.length ? <div className="react-diff-hscroll" ref={scrollBarRef} onScroll={onHScrollBarScroll}><div className="react-diff-hscroll-pad" style={{ width: hScrollWidth }} /></div> : null}</div>{requirements.error ? <ErrorCard error={requirements.error} /> : error ? <ErrorCard error={error} /> : loadingDiff ? <LoadingCard label="正在生成代码差异…" /> : files.length === 0 ? <EmptyCard>{snapshots.length === 0 ? <>暂无代码差异快照。点右上角「生成代码差异」首次生成并保存；之后每次刷新都会保留旧版本，可回退对比。</> : "该快照没有可展示的文件级差异。"}</EmptyCard> : <div className="react-diff-scroll-body" ref={scrollBodyRef} onScroll={onHScrollBodyScroll}>{files.map((item) => { const key = `${item.repo.repoName}:${item.file.path}`; const owners = hunkOwners(item.lines); const focusHunk = activeLine && activeLine.key === key ? owners[activeLine.line] ?? -1 : -1; return <article key={key} id={diffDomId(key)} className="react-diff-file-card"><header><div><FileCode2 size={16} /><strong>{item.repo.repoName}/{item.file.path}</strong><em className="react-diff-base-label">vs {item.repo.baseRef || review?.baseRef || "?"}</em></div><span><b>+{item.file.additions}</b><i>-{item.file.deletions}</i></span></header>{item.lines.length ? <table className="react-diff-code"><tbody>{item.lines.map((line, i) => <tr key={i} id={diffLineDomId(key, i)} className={`react-diff-line-${line.type}${focusHunk >= 0 && owners[i] === focusHunk ? " react-diff-line-focus" : ""}`} onClick={() => { setActiveKey(key); setActiveLine({ key, line: i }) }}><td>{line.oldNo}</td><td>{line.newNo}</td><td><code>{line.type === "add" ? "+" : line.type === "del" ? "-" : line.type === "hunk" ? "" : " "}{line.text || " "}</code></td></tr>)}</tbody></table> : <pre className="react-diff-preview">{item.diff || "该文件 diff 已截断或为空。"}</pre>}</article> })}</div>}</main>
      <AnnotationPanel
        fileLabel={activeView ? `${activeView.repo.repoName}/${activeView.file.path}` : ""}
        annotation={activeAnnotation}
        stale={Boolean(activeView && staleRepos.includes(activeView.repo.repoName))}
        activeNotes={hunkNotes.matched.filter((hit) => hit.lineIndex === activeHunkLine)}
        unmatchedNotes={hunkNotes.unmatched}
        onLocate={locateNote}
        onSave={saveAnnotation}
        saving={savingAnnotation}
 />
    </section>
  </PageChrome>
}
