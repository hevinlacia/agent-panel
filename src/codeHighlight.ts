/**
 * Role: server-side syntax highlighting for the code-review diff view, so
 *   changed lines render with IDEA-like token colors instead of plain text.
 * Public surface: detectHighlightLanguage, highlightDiffLines.
 * Constraints:
 *   - Output is pre-built HTML: highlight.js already escapes every text
 *     fragment (e.g. `List<String>` -> `List&lt;String&gt;`), so it is safe
 *     to inject raw into `<code>` via @kitajs/html (which renders string
 *     children unescaped by default). Never feed raw diff text straight to
 *     the page; always go through here or escapeHtml.
 *   - Multi-line constructs (block comments, text blocks, template literals)
 *     stay correctly colored by tokenizing the whole hunk at once and then
 *     re-splitting the highlighted HTML back into per-line fragments,
 *     rebalancing any `<span>` that crosses a newline.
 * Read-this-with: src/codeReview.ts (CodeReviewDiffLine), src/server.tsx
 *   (CodeReviewDiffRow / CodeReviewFilePanel) and the `.hljs-*` rules in
 *   public/style.css (Darcula-inspired palette).
 */
import hljs from "highlight.js"

/** Map source extensions/file names to highlight.js language ids. */
const EXT_LANG: Record<string, string> = {
  java: "java",
  kt: "kotlin",
  kts: "kotlin",
  scala: "scala",
  groovy: "groovy",
  gradle: "groovy",
  ts: "typescript",
  tsx: "tsx",
  js: "javascript",
  jsx: "jsx",
  mjs: "javascript",
  cjs: "javascript",
  xml: "xml",
  svg: "xml",
  xsd: "xml",
  xsl: "xml",
  xslt: "xml",
  html: "xml",
  htm: "xml",
  xhtml: "xml",
  vue: "xml",
  yml: "yaml",
  yaml: "yaml",
  json: "json",
  json5: "json",
  jsonc: "json",
  properties: "properties",
  ini: "ini",
  toml: "ini",
  conf: "ini",
  cfg: "ini",
  sql: "sql",
  py: "python",
  rb: "ruby",
  go: "go",
  rs: "rust",
  c: "c",
  h: "c",
  cpp: "cpp",
  cc: "cpp",
  cxx: "cpp",
  hpp: "cpp",
  hh: "cpp",
  cs: "csharp",
  php: "php",
  swift: "swift",
  sh: "bash",
  bash: "bash",
  zsh: "bash",
  md: "markdown",
  markdown: "markdown",
  css: "css",
  scss: "scss",
  less: "less",
  dockerfile: "dockerfile",
}

/** Files whose basename (case-insensitive) maps to a language directly. */
const NAME_LANG: Record<string, string> = {
  dockerfile: "dockerfile",
  makefile: "makefile",
  gnumakefile: "makefile",
  gemfile: "ruby",
  "rakefile": "ruby",
}

/**
 * Pick a highlight.js language for a file path, or `null` when the extension
 * is unknown / unsupported. Determined by extension so short diff snippets
 * are never mis-detected by auto-guessing.
 */
export function detectHighlightLanguage(filePath: string): string | null {
  const name = filePath.split("/").pop() || filePath
  const lower = name.toLowerCase()
  const byName = NAME_LANG[lower]
  if (byName && hljs.getLanguage(byName)) return byName
  const dot = lower.lastIndexOf(".")
  const ext = dot >= 0 ? lower.slice(dot + 1) : lower
  const lang = EXT_LANG[ext]
  if (!lang) return null
  return hljs.getLanguage(lang) ? lang : null
}

/** Hard cap to keep a pathological hunk from stalling page render. */
const MAX_HIGHLIGHT_LINES = 4000

/**
 * Split highlight.js HTML output into per-line HTML fragments, rebalancing
 * `<span>` tags that cross newlines (block comments, multi-line strings).
 *
 * highlight.js only emits `<span class="…">` / `</span>` and HTML-escaped
 * text, so a literal `<` only ever starts a span tag; `[^<\n]+` therefore
 * safely consumes whole text runs including `&lt;` entities.
 */
function splitHighlightHtmlByLine(html: string): string[] {
  const lines: string[] = []
  let buf = ""
  const stack: string[] = []
  const re = /<span class="([^"]*)">|<\/span>|\n|[^<\n]+|[\s\S]/g
  let m: RegExpExecArray | null
  while ((m = re.exec(html)) !== null) {
    const token = m[0]
    if (token === "\n") {
      // Close every currently-open span, emit the line, then reopen them so
      // the next line resumes inside the same comment/string context.
      for (let i = stack.length - 1; i >= 0; i--) buf += "</span>"
      lines.push(buf)
      buf = ""
      for (const cls of stack) buf += `<span class="${cls}">`
    } else if (token === "</span>") {
      stack.pop()
      buf += "</span>"
    } else if (m[1] !== undefined) {
      stack.push(m[1])
      buf += token
    } else {
      buf += token
    }
  }
  lines.push(buf)
  return lines
}

/** Escape text for safe use as HTML when not going through highlight.js. */
function escapeHtml(input: string): string {
  return input
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
}

/**
 * Highlight the code lines of a diff hunk, returning inner-HTML strings
 * aligned one-to-one with `lines` (one fragment per input line) for
 * injection into `<code>`.
 *
 * Behavior:
 *   - Unknown language, empty input, or oversized hunks fall back to
 *     plain escaped text (still safe, just uncolored).
 *   - `meta` lines (`\ No newline at end of file`) are kept as escaped text
 *     and excluded from tokenization so they don't pollute the code stream.
 *   - All other lines are joined with `\n`, tokenized once with the language
 *     grammar, then split back so multi-line comments/strings stay colored.
 */
export function highlightDiffLines(
  lines: ReadonlyArray<{ kind: string; content: string }>,
  lang: string | null,
): string[] {
  const fallback = lines.map((l) => escapeHtml(l.content || " "))
  if (!lang || lines.length === 0) return fallback

  const codeIdx: number[] = []
  const parts: string[] = []
  for (let i = 0; i < lines.length; i++) {
    if (lines[i].kind === "meta") continue
    codeIdx.push(i)
    parts.push(lines[i].content || "")
  }
  if (parts.length === 0 || parts.length > MAX_HIGHLIGHT_LINES) return fallback

  let value: string
  try {
    value = hljs.highlight(parts.join("\n"), { language: lang }).value
  } catch {
    return fallback
  }

  const highlighted = splitHighlightHtmlByLine(value)
  const result = fallback.slice()
  for (let j = 0; j < codeIdx.length && j < highlighted.length; j++) {
    result[codeIdx[j]] = highlighted[j]
  }
  return result
}
