/**
 * 剪贴板写入统一入口。
 *
 * 背景：`navigator.clipboard`（Async Clipboard API）仅在 secure context
 * （HTTPS 或 localhost）暴露；Agent Panel 通过 Tailscale 域名以 HTTP 访问时
 * 该属性为 undefined，直接调用会抛 TypeError，导致「复制命令/复制内容」等
 * 按钮静默失效（2026-10-22 用户报告）。
 *
 * 策略：secure context 优先走 Clipboard API（权限模型清晰、无 DOM 副作用）；
 * 否则回退隐藏 textarea + `document.execCommand("copy")`（已 deprecated，
 * 但是 HTTP 非 localhost 场景下唯一可行的程序化复制路径）。
 */
export async function copyTextToClipboard(text: string): Promise<void> {
  if (typeof navigator !== "undefined" && navigator.clipboard && window.isSecureContext) {
    await navigator.clipboard.writeText(text)
    return
  }
  const ta = document.createElement("textarea")
  ta.value = text
  // 不移出视口外（部分浏览器要求元素在可视区域内 select 才生效），仅视觉隐藏
  ta.style.position = "fixed"
  ta.style.top = "0"
  ta.style.left = "0"
  ta.style.opacity = "0"
  ta.style.pointerEvents = "none"
  document.body.appendChild(ta)
  ta.focus()
  ta.select()
  try {
    const ok = document.execCommand("copy")
    if (!ok) throw new Error("execCommand('copy') 返回 false")
  } finally {
    ta.remove()
  }
}
