/**
 * Role: renders a safe, lightweight Markdown subset to React nodes for the
 * requirement doc page. No HTML passthrough — raw HTML is escaped as plain text.
 * Public surface: <Markdown text={...} />.
 * Constraints: must not use dangerouslySetInnerHTML; must render arbitrary
 * agent-written content (headings, lists, tables, fenced code, links, emphasis).
 */
import { Fragment, type ReactNode } from "react"

function safeHref(href: string): string | null {
  const trimmed = href.trim()
  if (/^(https?:|mailto:)/i.test(trimmed)) return trimmed
  if (trimmed.startsWith("/") && !trimmed.startsWith("//")) return trimmed
  return null
}

const INLINE_RE =
  /(`[^`]+`)|(\*\*[^*\n]+\*\*)|(\*[^*\n]+\*)|(\[([^\]]+)\]\(([^)\s]+)\))|(~~[^~\n]+~~)/g

/** Parse inline markdown (code / bold / italic / link / strikethrough) into React nodes. */
function renderInline(text: string): ReactNode[] {
  const nodes: ReactNode[] = []
  const re = new RegExp(INLINE_RE.source, "g")
  let last = 0
  let m: RegExpExecArray | null
  let key = 0
  while ((m = re.exec(text))) {
    if (m.index > last) nodes.push(<Fragment key={key++}>{text.slice(last, m.index)}</Fragment>)
    const [full] = m
    if (m[1]) {
      nodes.push(<code key={key++} className="react-md-code">{m[1].slice(1, -1)}</code>)
    } else if (m[2]) {
      nodes.push(<strong key={key++}>{m[2].slice(2, -2)}</strong>)
    } else if (m[3]) {
      nodes.push(<em key={key++}>{m[3].slice(1, -1)}</em>)
    } else if (m[4]) {
      const href = safeHref(m[6] || "")
      const label = m[5] || full
      nodes.push(
        href
          ? <a key={key++} href={href} className="react-md-link" target={/^https?:/i.test(href) ? "_blank" : undefined} rel="noreferrer">{label}</a>
          : <span key={key++}>{label}</span>,
      )
    } else if (m[7]) {
      nodes.push(<s key={key++} className="react-md-del">{m[7].slice(2, -2)}</s>)
    }
    last = m.index + full.length
  }
  if (last < text.length) nodes.push(<Fragment key={key++}>{text.slice(last)}</Fragment>)
  return nodes
}

interface ListLeaf {
  content: string
  children: ListContainer
}
interface ListContainer {
  type: "ul" | "ol"
  children: ListLeaf[]
}

function renderListContainer(node: ListContainer, key: string | number): ReactNode {
  const items = node.children.map((leaf, index) => (
    <li key={index} className="react-md-li">
      {renderInline(leaf.content)}
      {leaf.children.children.length ? renderListContainer(leaf.children, `nested-${key}-${index}`) : null}
    </li>
  ))
  return node.type === "ul"
    ? <ul key={key} className="react-md-ul">{items}</ul>
    : <ol key={key} className="react-md-ol">{items}</ol>
}

const LIST_ITEM_RE = /^(\s*)([-*+]|\d+[.)])\s+(.*)$/

export function Markdown({ text }: { text: string }) {
  const lines = text.replace(/\r\n/g, "\n").split("\n")
  const blocks: ReactNode[] = []
  let i = 0
  let key = 0

  while (i < lines.length) {
    const line = lines[i]

    // fenced code block
    const fence = line.match(/^```([\w+-]*)\s*$/)
    if (fence) {
      const lang = fence[1] || "code"
      const buf: string[] = []
      i++
      while (i < lines.length && !/^```\s*$/.test(lines[i])) {
        buf.push(lines[i])
        i++
      }
      i++ // skip closing fence
      blocks.push(
        <pre key={key++} className="react-md-pre"><code className="react-md-pre-code">{buf.join("\n") || " "}</code></pre>,
      )
      continue
    }

    // heading
    const heading = line.match(/^(#{1,6})\s+(.*)$/)
    if (heading) {
      const level = heading[1].length
      const Tag = `h${level}` as "h1" | "h2" | "h3" | "h4" | "h5" | "h6"
      blocks.push(<Tag key={key++} className={`react-md-h react-md-h${level}`}>{renderInline(heading[2])}</Tag>)
      i++
      continue
    }

    // horizontal rule
    if (/^\s*([-*_])\1{2,}\s*$/.test(line)) {
      blocks.push(<hr key={key++} className="react-md-hr" />)
      i++
      continue
    }

    // blockquote
    if (/^\s*>\s?/.test(line)) {
      const buf: string[] = []
      while (i < lines.length && /^\s*>\s?/.test(lines[i])) {
        buf.push(lines[i].replace(/^\s*>\s?/, ""))
        i++
      }
      blocks.push(<blockquote key={key++} className="react-md-quote">{renderInline(buf.join("\n"))}</blockquote>)
      continue
    }

    // pipe table: header row + separator line
    if (
      line.trim().startsWith("|")
      && i + 1 < lines.length
      && /^\s*\|[\s:|-]+\|\s*$/.test(lines[i + 1])
      && lines[i + 1].includes("-")
    ) {
      const parseRow = (l: string) => l.trim().replace(/^\|/, "").replace(/\|$/, "").split("|").map((c) => c.trim())
      const header = parseRow(line)
      i += 2 // skip header + separator
      const rows: string[][] = []
      while (i < lines.length && lines[i].trim().startsWith("|")) {
        rows.push(parseRow(lines[i]))
        i++
      }
      blocks.push(
        <div key={key++} className="react-md-table-wrap">
          <table className="react-md-table">
            <thead><tr>{header.map((h, hi) => <th key={hi}>{renderInline(h)}</th>)}</tr></thead>
            <tbody>{rows.map((row, ri) => (
              <tr key={ri}>{header.map((_, ci) => <td key={ci}>{renderInline(row[ci] ?? "")}</td>)}</tr>
            ))}</tbody>
          </table>
        </div>,
      )
      continue
    }

    // lists (support nesting via indentation)
    if (LIST_ITEM_RE.test(line)) {
      const items: { type: "ul" | "ol"; depth: number; content: string }[] = []
      while (i < lines.length) {
        const lm = lines[i].match(LIST_ITEM_RE)
        if (!lm) break
        const indent = lm[1].replace(/\t/g, "  ").length
        const type = /^\d+/.test(lm[2]) ? "ol" : "ul"
        items.push({ type, depth: Math.floor(indent / 2), content: lm[3] })
        i++
        // lazy continuation lines belonging to this item
        while (
          i < lines.length
          && !LIST_ITEM_RE.test(lines[i])
          && lines[i].trim() !== ""
          && !/^\s*(#{1,6})\s/.test(lines[i])
          && !/^```/.test(lines[i])
        ) {
          items[items.length - 1].content += " " + lines[i].trim()
          i++
        }
      }
      const root: ListContainer = { type: "ul", children: [] }
      const stack: { depth: number; node: ListContainer }[] = [{ depth: -1, node: root }]
      for (const item of items) {
        while (stack.length > 1 && stack[stack.length - 1].depth >= item.depth) stack.pop()
        const parent = stack[stack.length - 1].node
        const leaf: ListLeaf = { content: item.content, children: { type: item.type, children: [] } }
        parent.children.push(leaf)
        stack.push({ depth: item.depth, node: leaf.children })
      }
      blocks.push(renderListContainer(root, key++))
      continue
    }

    // skip blank lines
    if (line.trim() === "") {
      i++
      continue
    }

    // paragraph
    const buf = [line]
    i++
    while (
      i < lines.length
      && lines[i].trim() !== ""
      && !/^(#{1,6})\s/.test(lines[i])
      && !/^```/.test(lines[i])
      && !LIST_ITEM_RE.test(lines[i])
      && !/^\s*>\s?/.test(lines[i])
      && !/^\s*\|/.test(lines[i])
      && !/^\s*([-*_])\1{2,}\s*$/.test(lines[i])
    ) {
      buf.push(lines[i])
      i++
    }
    blocks.push(<p key={key++} className="react-md-p">{renderInline(buf.join("\n"))}</p>)
  }

  return <div className="react-md">{blocks}</div>
}
