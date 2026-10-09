/**
 * Diff 页单个文件卡片：
 * - 未修改区间（文件头 / hunk 之间 / 文件尾）渲染为可展开标记，短文件（≤2000 行）自动全部展开，
 *   长文件按需分块展开，解决“文件展示不全、突然截断”的问题；
 * - 连续 import 区域默认折叠，点击展开；
 * - 行级点击选中/取消，支持多选；选中态与 hunk 无关。
 * 行锚点约束：备注/选中一律使用解析后 diff 行下标（view.lines 的 idx），
 * gap 展开行与折叠标记只是渲染层插入，不改变下标空间。
 */
import { forwardRef, useEffect, useImperativeHandle, useMemo, useRef, useState } from "react"
import { ChevronDown, ChevronRight, FileCode2 } from "lucide-react"
import type { CodeDiffSnapshot } from "../../types"
import { fetchJson } from "../../lib/api"
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
  | { kind: "gapLine"; no: number; text: string; drift: number }

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

  const rows = useMemo<RenderRow[]>(() => {
    const out: RenderRow[] = []
    const gapByFrom = new Map(allGaps.map((g) => [g.from, g]))
    const blockStarts = new Map(importBlocks.map((b) => [b.from, b]))
    const emitGapRows = (gap: FileGap) => {
      const st = gapStateOf(gapStates, gap.id)
      const chunks = [...st.chunks].sort((a, b) => a.from - b.from)
      let cursor = gap.from
      for (const chunk of chunks) {
        if (chunk.from > cursor) out.push({ kind: "gapToggle", gap, from: cursor, to: chunk.from - 1, loading: st.loading, error: st.error })
        for (let n = chunk.from; n <= chunk.to; n += 1) {
          out.push({ kind: "gapLine", no: n, text: chunk.lines[n - chunk.from] ?? "", drift: gap.drift })
        }
        cursor = Math.max(cursor, chunk.to + 1)
      }
      if (cursor <= gap.to) out.push({ kind: "gapToggle", gap, from: cursor, to: gap.to, loading: st.loading, error: st.error })
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
        out.push({ kind: "importToggle", blockId: block.id, count: block.to - block.from + 1, adds: block.adds, dels: block.dels, open: openImports.has(block.id) })
        if (openImports.has(block.id)) {
          for (let j = block.from; j <= block.to; j += 1) out.push({ kind: "diff", idx: j })
        }
        i = block.to + 1
        continue
      }
      out.push({ kind: "diff", idx: i })
      if (line.type === "add" || line.type === "ctx") prevNew = Number(line.newNo)
      i += 1
    }
    if (bottomGap) emitGapRows(bottomGap)
    return out
  }, [view, allGaps, importBlocks, openImports, gapStates, bottomGap])

  const truncatedRepo = Boolean(view.repo.diffTruncated)

  return <article id={diffDomId(key)} className="react-diff-file-card">
    <header>
      <div><FileCode2 size={16} /><strong>{view.repo.repoName}/{view.file.path}</strong><em className="react-diff-base-label">vs {view.repo.baseRef || review?.baseRef || "?"}</em></div>
      <span><b>+{view.file.additions}</b><i>-{view.file.deletions}</i></span>
    </header>
    {view.lines.length ? <table className="react-diff-code"><tbody>{rows.map((row, ri) => {
      if (row.kind === "diff") {
        const i = row.idx
        const line = view.lines[i]
        const rowKey = `${key}#${i}`
        return <tr
          key={`d${i}`}
          id={diffLineDomId(key, i)}
          className={`react-diff-line-${line.type}${rowKey === activeRowKey ? " react-diff-line-focus" : ""}${selectedSet.has(rowKey) ? " react-diff-line-selected" : ""}`}
          onClick={() => onToggleLine(key, i)}
        >
          <td>{line.oldNo}</td>
          <td>{line.newNo}</td>
          <td><code>{line.type === "add" ? "+" : line.type === "del" ? "-" : line.type === "hunk" ? "" : " "}{line.text || " "}</code></td>
        </tr>
      }
      if (row.kind === "importToggle") {
        return <tr key={`imp${ri}`} className="react-diff-gap-row react-diff-gap-import">
          <td colSpan={3}>
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
          <td colSpan={3}>
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
        <td><code> {row.text || " "}</code></td>
      </tr>
    })}</tbody></table> : <pre className="react-diff-preview">{truncatedRepo && !view.diff ? "该仓库 diff 超出输出上限，此文件内容未包含在快照中；可分仓或减小差异后重新生成。" : view.diff || "该文件 diff 已截断或为空。"}</pre>}
  </article>
})
