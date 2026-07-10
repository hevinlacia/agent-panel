/**
 * Role: Fastify request-context adapter so existing Hono-style handlers work
 *   unchanged on a native Fastify instance. Provides a Ctx type, an FC type, and
 *   a createRouter() helper that wraps fastify.get/post with context injection.
 * Public surface: FC, Ctx, createRouter.
 * Constraints: no Hono dependency; all response methods call reply.send() directly.
 * Read-this-with: src/server.tsx (the only consumer).
 */

import type { FastifyInstance, FastifyRequest, FastifyReply } from "fastify"

/** Minimal function-component type compatible with @kitajs/html string output. */
export type FC<P = Record<string, unknown>> = (props: P) => any

/** Hono-compatible context backed by Fastify request + reply. */
export interface Ctx {
  req: {
    query(key: string): string | undefined
    queries(key: string): string[]
    param(key: string): string
    json(): Promise<any>
    formData(): Promise<FormData>
    header(key: string): string | undefined
    readonly path: string
    readonly url: string
  }
  json(data: unknown, status?: number): void
  html(element: unknown, status?: number): void
  text(message: string, status?: number): void
  body(message: string, status?: number): void
  redirect(path: string, status?: number): void
}

/**
 * Convert a Fastify (request, reply) pair into a Ctx that the existing
 * Hono-authored handlers can use without modification. Each response method
 * calls reply.send() immediately so async handlers can `return c.json(...)`.
 */
function makeCtx(request: FastifyRequest, reply: FastifyReply): Ctx {
  const baseUrl = `${request.protocol}://${request.hostname}`
  const fullUrl = `${baseUrl}${request.url}`

  return {
    req: {
      query(key: string): string | undefined {
        const val = (request.query as Record<string, unknown>)[key]
        if (Array.isArray(val)) return String(val[0] ?? "")
        return val !== undefined && val !== null ? String(val) : undefined
      },
      queries(key: string): string[] {
        const val = (request.query as Record<string, unknown>)[key]
        if (Array.isArray(val)) return val.map(String)
        if (val !== undefined && val !== null) return [String(val)]
        return []
      },
      param(key: string): string {
        const val = (request.params as Record<string, unknown>)[key]
        return val !== undefined && val !== null ? String(val) : ""
      },
      async json(): Promise<any> {
        return request.body
      },
      async formData(): Promise<FormData> {
        const body = request.body as Record<string, unknown> | null
        const form = new FormData()
        if (!body || typeof body !== "object") return form
        for (const [key, value] of Object.entries(body)) {
          if (value && typeof (value as any).toBuffer === "function") {
            const buffer = await (value as any).toBuffer()
            const file = new File(
              [buffer],
              (value as any).filename || "upload",
              { type: (value as any).mimetype || "application/octet-stream" },
            )
            form.append(key, file)
          } else if (value !== undefined && value !== null) {
            form.append(key, String(value))
          }
        }
        return form
      },
      header(key: string): string | undefined {
        const val = request.headers[key.toLowerCase()]
        if (Array.isArray(val)) return val[0]
        return val !== undefined ? String(val) : undefined
      },
      get path() {
        return request.url.split("?")[0]
      },
      get url() {
        return fullUrl
      },
    },
    json(data: unknown, status?: number) {
      reply.code(status ?? 200).send(data)
    },
    html(element: unknown, status?: number) {
      reply.code(status ?? 200).type("text/html; charset=utf-8").send(element)
    },
    text(message: string, status?: number) {
      reply.code(status ?? 200).type("text/plain; charset=utf-8").send(message)
    },
    body(message: string, status?: number) {
      reply.code(status ?? 200).send(message)
    },
    redirect(path: string, status?: number) {
      reply.code(status ?? 302).header("Location", path).send("")
    },
  }
}

/** Route registration helper that mirrors Hono's app.get/app.post API. */
export function createRouter(fastify: FastifyInstance) {
  return {
    get(path: string, handler: (c: Ctx) => Promise<unknown> | unknown) {
      fastify.get(path, async (request, reply) => {
        await handler(makeCtx(request, reply))
      })
    },
    post(path: string, handler: (c: Ctx) => Promise<unknown> | unknown) {
      fastify.post(path, async (request, reply) => {
        await handler(makeCtx(request, reply))
      })
    },
  }
}

export type Router = ReturnType<typeof createRouter>
