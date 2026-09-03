import { postForm } from "../../lib/api"
import type { NewSessionPayload } from "../../types"

export interface SessionCommandResult {
  command: string
  sessionId: string
  /** true = 复用了未使用过的 pending session id；false = 刚生成的新 id。 */
  reused: boolean
}

/**
 * 取需求最新的终端启动命令并写入剪贴板。
 *
 * 服务端按 pending 复用语义处理：之前生成的 session id 还没被用过（session
 * 库里还没有对应 session 文件）时原样返回旧命令；已被使用过则自动换新
 * session id；`force` 无视使用状态强制换新。
 *
 * 仅 pi / dsh-tui harness 会生成命令；dsh-web 抛错（调用方应隐藏入口）。
 */
export async function copyRequirementSessionCommand(
  reqId: string,
  opts?: { force?: boolean },
): Promise<SessionCommandResult> {
  const body: Record<string, string> = { reqId }
  if (opts?.force) body.force = "true"
  const res = await postForm<NewSessionPayload>("/api/requirement/new-session", body)
  if (!res.command) throw new Error("当前 harness 不生成终端命令")
  await navigator.clipboard.writeText(res.command)
  return { command: res.command, sessionId: res.sessionId || "", reused: Boolean(res.reused) }
}
