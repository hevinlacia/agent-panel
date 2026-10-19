/**
 * Agent Panel service worker（最小化）：
 * - 仅 cache-first 缓存 /assets/ 下的 hash 版本化静态文件；
 * - API/WS/页面导航全部网络直连（面板是实时数据应用，不做离线兜底）；
 * - 更新策略：部署后新 SW 字节变化 -> install skipWaiting -> activate 清旧缓存。
 */
const CACHE = "panel-assets-v1"

self.addEventListener("install", () => {
  self.skipWaiting()
})

self.addEventListener("activate", (event) => {
  event.waitUntil((async () => {
    const names = await caches.keys()
    await Promise.all(names.filter((n) => n !== CACHE).map((n) => caches.delete(n)))
    await self.clients.claim()
  })())
})

self.addEventListener("fetch", (event) => {
  const req = event.request
  if (req.method !== "GET") return
  const url = new URL(req.url)
  if (url.origin !== self.location.origin) return
  if (!url.pathname.startsWith("/assets/")) return
  event.respondWith((async () => {
    const cache = await caches.open(CACHE)
    const hit = await cache.match(req)
    if (hit) return hit
    const res = await fetch(req)
    if (res.ok) cache.put(req, res.clone())
    return res
  })())
})
