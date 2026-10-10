/**
 * Diff 页单个文件卡片：
 * - 未修改区间（文件头 / hunk 之间 / 文件尾）渲染为可展开标记，短文件（≤2000 行）自动全部展开，
 *   长文件按需分块展开，解决“文件展示不全、突然截断”的问题；
 * - 连续 import 区域默认折叠，点击展开；
 * - 行级点击选中/取消，支持多选；选中态与 hunk 无关；
 * - 代码行语法高亮（shiki）：连续可高亮行整体分词（hunk 头/标记行不入流不断流，
 *   gap 展开后 token 流与文件真实行序连续，跨行注释/字符串状态正确），失败降级纯文本。
 * 行锚点约束：备注/选中一律使用解析后 diff 行下标（view.lines 的 idx），
 * gap 展开行与折叠标记只是渲染层插入，不改变下标空间。
 */
import { forwardRef, useEffect, useImperativeHandle, useMemo, useRef, useState } from "react"
import { ChevronDown, ChevronRight, FileCode2, Users } from "lucide-react"
import type { CodeDiffSnapshot } from "../../types"
import { fetchJson } from "../../lib/api"
import { inferDiffLanguage, highlightUnits, type HighlightUnit, type ThemedToken } from "../../lib/highlight"
import {
  buildImportBlocks,
  computeFileGaps,
  diffDomId,
  diffLineDomId,
  type DiffFileView,
  type FileGap,
} from "../../lib/diff"

/** 文件不超过该行数时视为短文件：自动展开所有未修改区间，整文件可见。 */
const SHORT_FILE_FULL_LINES = 2000
/** 长文件单次展开的最大行数（与后端 /api/git/file-lines 单次上限对齐）。 */
const GAP_FETCH_CHUNK = 2400

interface FileLinesPayload {
  ok: boolean
  from: number
  to: number
  totalLines: number
  binary: boolean
  lines: string[]
}

interface BlameInfo { author: string; date: string }

interface BlamePayload {
  ok: boolean
  totalLines: number
  truncated: boolean
  lines: { no: number; author: string; date: string }[]
}

interface GapChunk { from: number; to: number; lines: string[] }

interface GapState {
  chunks: GapChunk[]
  loading: boolean
  error: string | null
}

type RenderRow =
  | { kind: "diff"; idx: number }
  | { kind: "importToggle"; blockId: string; count: number; adds: number; dels: number; open: boolean }
  | { kind: "gapToggle"; gap: FileGap; from: number; to: number; loading: boolean; error: string | null }
  | { kind: "gapLine"; gapId: string; no: number; text: string; drift: number }

/** 高亮行渲染：token 有颜色/字重才包 span，其余原样输出（内容已转义）。 */
function TokenSpans({ tokens }: { tokens: ThemedToken[] }) {
  return <>{tokens.map((t, i) => {
    const fs = t.fontStyle ?? 0
    return (t.color || fs)
      ? <span key={i} style={{ color: t.color, fontStyle: fs & 1 ? "italic" : undefined, fontWeight: fs & 2 ? 700 : undefined }}>{t.content}</span>
      : t.content
  })}</>
}

export interface DiffFileCardHandle {
  /** 展开目标行所在的折叠 import 区块（如已折叠）并把行滚动到视口中间。 */
  revealLine: (idx: number) => void
}

interface DiffFileCardProps {
  view: DiffFileView
  review: CodeDiffSnapshot | null
  /** 全局行选中集合：`${fileKey}#${lineIdx}` */
  selectedSet: Set<string>
  /** 当前行（备注面板锚点行），单独描边显示 */
  activeRowKey: string | null
  onToggleLine: (key: string, line: number) => void
}

function gapStateOf(states: Record<string, GapState>, id: string): GapState {
  return states[id] || { chunks: [], loading: false, error: null }
}

function coveredChunks(chunks: GapChunk[], from: number, to: number): boolean {
  if (to < from) return true
  let cursor = from
  for (const c of [...chunks].sort((a, b) => a.from - b.from)) {
    if (c.from > cursor) return false
    cursor = Math.max(cursor, c.to + 1)
    if (cursor > to) return true
  }
  return cursor > to
}

export const DiffFileCard = forwardRef<DiffFileCardHandle, DiffFileCardProps>(function DiffFileCard(
  { view, review, selectedSet, activeRowKey, onToggleLine },
  ref,
) {
  const key = `${view.repo.repoName}:${view.file.path}`
  const [meta, setMeta] = useState<{ totalLines: number; binary: boolean } | null>(null)
  // 行信息（blame）：点击工具栏按钮展开每行修改人/修改日期，再点收起（数据保留不重复请求）。
  const [blameOn, setBlameOn] = useState(false)
  const [blameMap, setBlameMap] = useState<Record<number, BlameInfo> | null>(null)
  const [blameLoading, setBlameLoading] = useState(false)
  const [blameError, setBlameError] = useState<string | null>(null)
  const [gapStates, setGapStates] = useState<Record<string, GapState>>({})
  const [openImports, setOpenImports] = useState<Set<string>>(() => new Set())

  const scan = useMemo(() => computeFileGaps(view.lines), [view.lines])
  const importBlocks = useMemo(() => buildImportBlocks(view.lines), [view.lines])
  const bottomGap = useMemo<FileGap | null>(() => {
    if (!meta || meta.binary || !scan.hasNewSide || meta.totalLines <= scan.lastNew) return null
    return { id: "gap-bottom", from: scan.lastNew + 1, to: meta.totalLines, drift: scan.lastDrift }
  }, [meta, scan])
  const allGaps = useMemo(() => (bottomGap ? [...scan.gaps, bottomGap] : scan.gaps), [scan, bottomGap])

  const metaRequested = useRef(false)
  useEffect(() => {
    if (metaRequested.current) return
    metaRequested.current = true
    const projectPath = view.repo.projectPath
    const commit = view.repo.targetCommit
    // 纯删除文件（新侧不存在）或缺少定位信息时不拉取文件内容。
    if (!projectPath || !commit || !scan.hasNewSide) return
    const qs = new URLSearchParams({ projectPath, commit, path: view.file.path, from: "1", to: "1" })
    fetchJson<FileLinesPayload>(`/api/git/file-lines?${qs.toString()}`)
      .then((p) => setMeta({ totalLines: p.totalLines, binary: p.binary }))
      .catch(() => setMeta(null))
    // view 在快照切换时整体替换，重置由 key/remount 保证。
  }, [])

  const expandGap = (gap: FileGap, full: boolean) => {
    const from = gap.from
    const to = full ? gap.to : Math.min(from + GAP_FETCH_CHUNK - 1, gap.to)
    setGapStates((prev) => {
      const st = gapStateOf(prev, gap.id)
      if (st.loading || coveredChunks(st.chunks, from, to)) return prev
      return { ...prev, [gap.id]: { ...st, loading: true, error: null } }
    })
    const projectPath = view.repo.projectPath
    const commit = view.repo.targetCommit
    if (!projectPath || !commit) {
      setGapStates((prev) => ({ ...prev, [gap.id]: { ...gapStateOf(prev, gap.id), loading: false, error: "缺少仓库路径或 commit 信息，无法展开" } }))
      return
    }
    const qs = new URLSearchParams({ projectPath, commit, path: view.file.path, from: String(from), to: String(to) })
    fetchJson<FileLinesPayload>(`/api/git/file-lines?${qs.toString()}`)
      .then((p) => {
        setGapStates((prev) => {
          const st = gapStateOf(prev, gap.id)
          return { ...prev, [gap.id]: { chunks: [...st.chunks, { from: p.from, to: p.to, lines: p.lines }], loading: false, error: null } }
        })
      })
      .catch((err) => {
        setGapStates((prev) => ({ ...prev, [gap.id]: { ...gapStateOf(prev, gap.id), loading: false, error: err instanceof Error ? err.message : String(err) } }))
      })
  }

  // 短文件自动展开全部未修改区间 => 整文件可见；长文件保持折叠按需展开。
  const autoExpanded = useRef(false)
  useEffect(() => {
    if (autoExpanded.current || !meta || meta.binary) return
    if (meta.totalLines > SHORT_FILE_FULL_LINES) return
    autoExpanded.current = true
    for (const gap of allGaps) expandGap(gap, true)
  }, [meta])

  const toggleImport = (blockId: string) => {
    setOpenImports((prev) => {
      const next = new Set(prev)
      if (next.has(blockId)) next.delete(blockId)
      else next.add(blockId)
      return next
    })
  }

  const pendingScroll = useRef<number | null>(null)
  const revealLine = (idx: number) => {
    const block = importBlocks.find((b) => idx >= b.from && idx <= b.to)
    if (block && !openImports.has(block.id)) {
      setOpenImports((prev) => new Set(prev).add(block.id))
    }
    pendingScroll.current = idx
  }
  useImperativeHandle(ref, () => ({ revealLine }))
  useEffect(() => {
    if (pendingScroll.current == null) return
    const idx = pendingScroll.current
    pendingScroll.current = null
    document.getElementById(diffLineDomId(key, idx))?.scrollIntoView({ behavior: "smooth", block: "center" })
  })

  // 行渲染序列 + 语法高亮单元：连续可高亮行（diff 非 hunk 行 / gap 展开行）组成一个单元整体分词，
  // hunk 头与各类标记行不参与也不打断单元——gap 展开后 token 流与文件真实行序连续，状态跨 gap 保持正确。
  const rendered = useMemo(() => {
    const rows: RenderRow[] = []
    const units: HighlightUnit[] = []
    const gapByFrom = new Map(allGaps.map((g) => [g.from, g]))
    const blockStarts = new Map(importBlocks.map((b) => [b.from, b]))
    // 复用单元缓冲（unitActive 标记开启），避免闭包内可空变量的 TS 收窄问题。
    const unit: { rowKeys: string[]; texts: string[] } = { rowKeys: [], texts: [] }
    let unitActive = false
    const pushTokenizable = (rowKey: string, text: string) => {
      if (!unitActive) {
        unit.rowKeys.length = 0
        unit.texts.length = 0
        unitActive = true
      }
      unit.rowKeys.push(rowKey)
      unit.texts.push(text)
    }
    const flushUnit = () => {
      if (!unitActive) return
      units.push({ rowKeys: [...unit.rowKeys], text: unit.texts.join("\n") })
      unitActive = false
    }
    const pushDiffRow = (idx: number) => {
      const line = view.lines[idx]
      rows.push({ kind: "diff", idx })
      if (line.type !== "hunk") pushTokenizable(`d:${idx}`, line.text)
    }
    const emitGapRows = (gap: FileGap) => {
      const st = gapStateOf(gapStates, gap.id)
      const chunks = [...st.chunks].sort((a, b) => a.from - b.from)
      let cursor = gap.from
      for (const chunk of chunks) {
        if (chunk.from > cursor) {
          flushUnit()
          rows.push({ kind: "gapToggle", gap, from: cursor, to: chunk.from - 1, loading: st.loading, error: st.error })
        }
        for (let n = chunk.from; n <= chunk.to; n += 1) {
          const text = chunk.lines[n - chunk.from] ?? ""
          rows.push({ kind: "gapLine", gapId: gap.id, no: n, text, drift: gap.drift })
          pushTokenizable(`g:${gap.id}:${n}`, text)
        }
        cursor = Math.max(cursor, chunk.to + 1)
      }
      if (cursor <= gap.to) {
        flushUnit()
        rows.push({ kind: "gapToggle", gap, from: cursor, to: gap.to, loading: st.loading, error: st.error })
      }
    }
    let prevNew = 0
    let i = 0
    while (i < view.lines.length) {
      const line = view.lines[i]
      // 先检查该行之前是否存在未展示区间（文件头 gap / hunk 间 gap）
      let pending: FileGap | null = null
      if (line.type === "hunk") {
        const m = line.text.match(/^@@ -\d+(?:,\d+)? \+(\d+)(?:,\d+)? @@/)
        const newStart = m ? Number(m[1]) : NaN
        if (Number.isFinite(newStart) && newStart > prevNew + 1) pending = gapByFrom.get(prevNew + 1) ?? null
      } else if (line.type === "add" || line.type === "ctx") {
        const newNo = Number(line.newNo)
        if (Number.isFinite(newNo) && newNo > prevNew + 1) pending = gapByFrom.get(prevNew + 1) ?? null
      }
      if (pending) {
        emitGapRows(pending)
        prevNew = pending.to
        continue
      }
      const block = blockStarts.get(i)
      if (block) {
        flushUnit()
        rows.push({ kind: "importToggle", blockId: block.id, count: block.to - block.from + 1, adds: block.adds, dels: block.dels, open: openImports.has(block.id) })
        if (openImports.has(block.id)) {
          for (let j = block.from; j <= block.to; j += 1) pushDiffRow(j)
        }
        i = block.to + 1
        continue
      }
      pushDiffRow(i)
      if (line.type === "add" || line.type === "ctx") prevNew = Number(line.newNo)
      i += 1
    }
    flushUnit()
    if (bottomGap) emitGapRows(bottomGap)
    return { rows, units }
  }, [view, allGaps, importBlocks, openImports, gapStates, bottomGap])
  const rows = rendered.rows

  // 语法高亮：按文件扩展名推断语言，异步分词后按 rowKey 注入；内容签名去重避免重复分词。
  const lang = useMemo(() => inferDiffLanguage(view.file.path), [view.file.path])
  const [tokenMap, setTokenMap] = useState<Record<string, ThemedToken[]>>({})
  const tokenSigRef = useRef("")
  const highlightSig = useMemo(() => {
    if (!lang || !rendered.units.length) return ""
    let h = 5381
    for (const u of rendered.units) {
      h = (h * 33) ^ u.rowKeys.length
      for (let i = 0; i < u.text.length; i += 1) h = (h * 33) ^ u.text.charCodeAt(i)
    }
    return `${lang}:${rendered.units.length}:${h >>> 0}`
  }, [lang, rendered.units])
  useEffect(() => {
    if (!lang || !rendered.units.length || !highlightSig) return
    if (tokenSigRef.current === highlightSig) return
    let cancelled = false
    highlightUnits(lang, rendered.units).then((map) => {
      if (cancelled) return
      tokenSigRef.current = highlightSig
      setTokenMap((prev) => ({ ...prev, ...map }))
    })
    return () => { cancelled = true }
  })

  const toggleBlame = () => {
    const next = !blameOn
    setBlameOn(next)
    if (!next || blameMap || blameLoading) return
    const projectPath = view.repo.projectPath
    const commit = view.repo.targetCommit
    if (!projectPath || !commit) {
      setBlameError("缺少仓库路径或 commit 信息，无法加载行信息")
      return
    }
    setBlameLoading(true)
    setBlameError(null)
    const qs = new URLSearchParams({ projectPath, commit, path: view.file.path })
    fetchJson<BlamePayload>(`/api/git/blame?${qs.toString()}`)
      .then((p) => {
        const map: Record<number, BlameInfo> = {}
        for (const l of p.lines || []) map[l.no] = { author: l.author, date: l.date }
        setBlameMap(map)
      })
      .catch((err) => setBlameError(err instanceof Error ? err.message : String(err)))
      .finally(() => setBlameLoading(false))
  }

  const blameCell = (newNo: string) => {
    if (!blameOn) return null
    const no = Number(newNo)
    const info = Number.isFinite(no) ? blameMap?.[no] : undefined
    return <td className="react-diff-blame-cell">{info ? <span title={`${info.author} · ${info.date}`}>{info.author}{info.date ? ` · ${info.date.slice(5)}` : ""}</span> : <span className="react-diff-blame-empty">·</span>}</td>
  }

  const truncatedRepo = Boolean(view.repo.diffTruncated)

  return <article id={diffDomId(key)} className="react-diff-file-card">
    <header>
      <div><FileCode2 size={16} /><strong>{view.repo.repoName}/{view.file.path}</strong><em className="react-diff-base-label">vs {view.repo.baseRef || review?.baseRef || "?"}</em></div>
      <span className="react-diff-head-right"><b>+{view.file.additions}</b><i>-{view.file.deletions}</i><button type="button" className={`react-diff-blame-btn ${blameOn ? "is-on" : ""}`} onClick={toggleBlame} title={blameOn ? "收起行信息（修改人/修改日期）" : "查看每行的修改人与修改时间（基于目标分支版本 blame）"} disabled={blameLoading}><Users size={13} />{blameLoading ? "加载中…" : "行信息"}</button></span>
    </header>
    {blameOn && blameError ? <p className="react-effort-error react-diff-blame-error">行信息加载失败：{blameError}</p> : null}
    {view.lines.length ? <table className="react-diff-code"><tbody>{rows.map((row, ri) => {
      if (row.kind === "diff") {
        const i = row.idx
        const line = view.lines[i]
        const rowKey = `${key}#${i}`
        const prefix = line.type === "add" ? "+" : line.type === "del" ? "-" : line.type === "hunk" ? "" : " "
        const tokens = line.type === "hunk" ? undefined : tokenMap[`d:${i}`]
        return <tr
          key={`d${i}`}
          id={diffLineDomId(key, i)}
          className={`react-diff-line-${line.type}${rowKey === activeRowKey ? " react-diff-line-focus" : ""}${selectedSet.has(rowKey) ? " react-diff-line-selected" : ""}`}
          onClick={() => onToggleLine(key, i)}
        >
          <td>{line.oldNo}</td>
          <td>{line.newNo}</td>
          <td><code>{prefix}{line.text ? (tokens ? <TokenSpans tokens={tokens} /> : line.text) : " "}</code></td>
          {blameCell(line.type === "del" ? "" : line.newNo)}
        </tr>
      }
      if (row.kind === "importToggle") {
        return <tr key={`imp${ri}`} className="react-diff-gap-row react-diff-gap-import">
          <td colSpan={blameOn ? 4 : 3}>
            <button onClick={() => toggleImport(row.blockId)}>
              {row.open ? <ChevronDown size={13} /> : <ChevronRight size={13} />}
              import 区块（{row.count} 行{row.adds ? ` · +${row.adds}` : ""}{row.dels ? ` · -${row.dels}` : ""}）· 点击{row.open ? "折叠" : "展开"}
            </button>
          </td>
        </tr>
      }
      if (row.kind === "gapToggle") {
        const count = row.to - row.from + 1
        return <tr key={`gap${ri}`} className="react-diff-gap-row">
          <td colSpan={blameOn ? 4 : 3}>
            <button onClick={() => expandGap(row.gap, false)} disabled={row.loading}>
              {row.error
                ? <>展开失败：{row.error} · 点击重试</>
                : row.loading
                  ? <>正在加载第 {row.from} – {row.to} 行…</>
                  : <>⋯ 未修改的第 {row.from} – {row.to} 行（{count} 行）· 点击展开 ⋯</>}
            </button>
          </td>
        </tr>
      }
      return <tr key={`gl${ri}`} className="react-diff-line-ctx react-diff-line-gap">
        <td>{row.no - row.drift > 0 ? row.no - row.drift : ""}</td>
        <td>{row.no}</td>
        <td><code>{" "}{row.text ? (tokenMap[`g:${row.gapId}:${row.no}`] ? <TokenSpans tokens={tokenMap[`g:${row.gapId}:${row.no}`]} /> : row.text) : " "}</code></td>
        {blameCell(String(row.no))}
      </tr>
    })}</tbody></table> : <pre className="react-diff-preview">{truncatedRepo && !view.diff ? "该仓库 diff 超出输出上限，此文件内容未包含在快照中；可分仓或减小差异后重新生成。" : view.diff || "该文件 diff 已截断或为空。"}</pre>}
  </article>
})
