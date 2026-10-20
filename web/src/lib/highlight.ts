/**
 * Diff 页代码语法高亮（Shiki + JS 正则引擎，无 WASM）：
 * - 语言按文件扩展名推断；语法包按需动态加载（Vite 代码分片），未支持语言静默降级纯文本；
 * - 分词以「连续可高亮行」为单元整体进行（跨行的块注释/模板字符串等语法状态正确），
 *   hunk 头/折叠标记行打断单元（文件行流在 gap 处本就不连续）；
 * - token 结果按 `${lang}\0${text}` 缓存，gap 展开/重新渲染只增量化。
 */
import { bundledLanguages, createHighlighter, type BundledLanguage, type Highlighter, type ThemedToken } from "shiki"
import { createJavaScriptRegexEngine } from "shiki/engine/javascript"

export const SHIKI_THEME = "github-dark-default"

export type { ThemedToken }

export interface HighlightUnit {
  /** 该单元覆盖的渲染行 key（diff 行 `d:<idx>` / gap 行 `g:<gapId>:<no>`），与 text 行一一对应 */
  rowKeys: string[]
  /** 各行代码文本（不含 diff +/- 前缀），按 \n 连接后整体分词 */
  text: string
}

/** 常见扩展名 -> shiki 语言 id */
const EXT_LANG: Record<string, string> = {
  java: "java",
  groovy: "groovy",
  gradle: "groovy",
  kt: "kotlin",
  kts: "kotlin",
  xml: "xml",
  yml: "yaml",
  yaml: "yaml",
  properties: "properties",
  ts: "typescript",
  mts: "typescript",
  cts: "typescript",
  tsx: "tsx",
  js: "javascript",
  mjs: "javascript",
  cjs: "javascript",
  jsx: "jsx",
  json: "json",
  sql: "sql",
  py: "python",
  go: "go",
  rs: "rust",
  sh: "shellscript",
  bash: "shellscript",
  md: "markdown",
  markdown: "markdown",
  css: "css",
  scss: "scss",
  less: "less",
  html: "html",
  htm: "html",
  vue: "vue",
  c: "c",
  h: "c",
  cpp: "cpp",
  cc: "cpp",
  hpp: "cpp",
  cs: "csharp",
  scala: "scala",
  dart: "dart",
  proto: "proto",
  php: "php",
  rb: "ruby",
  swift: "swift",
}

export function inferDiffLanguage(path: string): string | undefined {
  const name = (path.split("/").pop() || "").toLowerCase()
  if (name === "dockerfile") return "dockerfile"
  const ext = name.includes(".") ? name.slice(name.lastIndexOf(".") + 1) : ""
  const lang = EXT_LANG[ext]
  return lang && lang in bundledLanguages ? lang : undefined
}

let highlighterPromise: Promise<Highlighter> | null = null

function getHighlighter(): Promise<Highlighter> {
  if (!highlighterPromise) {
    highlighterPromise = createHighlighter({
      themes: [SHIKI_THEME],
      langs: [],
      // JS 正则引擎：不引入 oniguruma WASM；forgiving 让个别未移植语法静默降级而非抛错。
      engine: createJavaScriptRegexEngine({ forgiving: true }),
    }).catch((err) => {
      highlighterPromise = null
      throw err
    })
  }
  return highlighterPromise
}

const tokenCache = new Map<string, ThemedToken[][]>()

/** 批量高亮单元，返回 rowKey -> 该行 token 数组；失败返回空对象（调用方按纯文本渲染）。 */
export async function highlightUnits(lang: string, units: HighlightUnit[]): Promise<Record<string, ThemedToken[]>> {
  const out: Record<string, ThemedToken[]> = {}
  if (!units.length) return out
  try {
    const highlighter = await getHighlighter()
    if (!highlighter.getLoadedLanguages().includes(lang)) {
      const bundle = bundledLanguages[lang as BundledLanguage]
      if (!bundle) return out
      await highlighter.loadLanguage(await bundle())
    }
    for (const unit of units) {
      const cacheKey = `${lang}\u0000${unit.text}`
      let lineTokens = tokenCache.get(cacheKey)
      if (!lineTokens) {
        lineTokens = highlighter.codeToTokensBase(unit.text, { lang: lang as BundledLanguage, theme: SHIKI_THEME })
        if (tokenCache.size > 600) tokenCache.clear()
        tokenCache.set(cacheKey, lineTokens)
      }
      // 行数与渲染行不匹配时该单元整体保持纯文本，避免错位。
      if (lineTokens.length !== unit.rowKeys.length) continue
      unit.rowKeys.forEach((rowKey, i) => { out[rowKey] = lineTokens![i] })
    }
  } catch {
    // 高亮失败降级为纯文本
  }
  return out
}
