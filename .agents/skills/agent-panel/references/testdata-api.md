# Testdata API（接口测试联动 + 造数能力执行）

> 上级入口：`../SKILL.md`。数据源是 WMS testdata pack（`$WMS_WORKSPACE_ROOT/.agents/testdata/`：api/catalog.yaml、api/bruno/、api/profiles.yaml、capabilities.yaml）。

用于：通过 Agent Panel 检索可复用接口模板、构造并触发 WMS 测试/UAT 请求（dry-run 预览 + 真实执行）、检索并执行造数能力。panel 是一条便捷执行通道；业务流程与造数编排仍以 `wms-test-data-creation` / `wms-test-api-call` skill 为准（本 API 与其能力同源，不重复造数规则）。

适用：
- 测接口前想复用已维护模板：先查目录、看详情、拿构造预览。
- 造数前低 token 确认能力/目标状态/CLI 参数，或经 panel 统一 dispatch 执行造数脚本。
- 需要三环境（test/uat-cn/uat-sea）自动构造网关域名、必需头、登录态的请求。

不适用：
- panel 没有维护模板的接口（走 `wms-test-api-call` 推断构造，验证后回填 catalog + .bru）。
- 造数状态图/路径规划（`wms-test-data-creation`）。
- UAT 写库（红线：UAT 禁止 DB 写；本 API 只发 HTTP 请求，DB 兜底仅限 test 且走 skill 流程）。

## 端点总览

| 方法 路径 | 用途 |
|---|---|
| GET `/api/apitest/apis` | 接口目录（造数/测试标记 + 关键字过滤） |
| GET `/api/apitest/api?id=<api-id>` | 单接口详情：模板 headers/body/varsMap、占位符、schema |
| POST `/api/apitest/trigger` | 构造并触发请求（dry-run 预览 / 真实执行） |
| GET `/api/capabilities` | 造数能力清单（别名 `/api/testdata/capabilities`、`/api/agent/capabilities/query`） |
| GET `/api/capability?id=<id>&project=WMS` | 造数能力详情（targets/cli/pitfalls/notes） |
| GET `/api/capability/sources` | testdata pack 挂载状态 |
| POST `/api/testdata/run` | 造数脚本 dry-run / 执行 dispatch 唯一入口 |

## 接口目录与详情

**GET `/api/apitest/apis`**：query 均可选——`q`（关键字，匹配 id/name/purpose/url/tags）、`domain`（精确）、`kind`（`create`/`test`/`all`，默认 all）、`status`（精确）。返回 `{ok, count, apis:[{id, domain, purpose, method, url, status, kind, tags, envSupport, notes, hasRequestSchema, placeholderCount, templateOk}], packRoot}`。method/url 优先取 .bru 解析值，模板缺失回退 catalog 字段。

**GET `/api/apitest/api?id=<api-id>`**：返回 `{ok, api, template:{headers, body, varsMap}, tests, placeholders:[{name, envVar, derived}], requestSchema, requestSchemaPath, responseSchemaPath}`。`derived=true` 表示该占位符由会话/环境自动派生（envVar ∈ {WMS_X_TOKEN, WMS_X_CLIENT, WMS_X_PARAMS, WMS_X_USER, WMS_X_DB, WMS_BASE_URL}），无需手动传。

## 触发请求：POST `/api/apitest/trigger`

Body（JSON 或表单）：

| 参数 | 必填 | 说明 |
|---|---|---|
| `id` | 是 | 接口 id（catalog.yaml） |
| `env` | 否 | `test`（默认）/ `uat-cn` / `uat-sea` |
| `vars` | 否 | `Record<string,string>`，按占位符 canonical 名或 env 名匹配 |
| `bodyOverride` | 否 | 整体替换请求 body（内部 `{{占位符}}` 仍会被替换） |
| `dryRun` | 否 | 默认 false |

响应语义：

- **缺占位符**：HTTP 200 `{ok:false, missing:[...], message}`，不发请求；补 vars 重试。
- **dry-run**：`{ok:true, dryRun:true, env, apiId, method, url, headers, body, tests, note}`——完整走登录态获取 + 构造，只返回预览不发送；`X-Token`/`X-Client` 脱敏为 `***redacted(len=N)`。
- **真实执行**：`{ok:(HTTP 200-399), status, durationMs, contentType, url, bodyText, bodyJson, truncated, env, apiId, kind, tests, note}`；响应不含请求头。单次超时 60s（外层 65s），响应体 >256KB 截断（`truncated:true`）。
- 错误：400 未知环境/接口不存在/模板读取失败/登录脚本缺失；登录态获取失败提示 Chrome 登录 WMS。

登录态：子进程调 `wms-auth-auto-login/login.mjs --env <env>`（90s 超时）。token/client 只用于后端发出的请求头——dry-run 中脱敏、真实响应不回显、不落盘、不进日志。

## profiles.yaml 环境契约

文件：`{pack root}/api/profiles.yaml`，缺失/损坏时用内置 fallback（结构相同）。agent 手工构造请求时可直接参考该契约，不必重复推导：

```yaml
environments:
  <test|uat-cn|uat-sea>:
    gateway: <业务接口网关域名>          # 必填
    pda_gateway: <可选，仅 uat-sea>      # URL path 以 /wms-web 开头时使用（SEA UAT 双域名）
    origin / referer: <必填>
    x_env: <可选>                        # test="test"、uat-sea="uat"、uat-cn 无
    x_params:                            # 可选；缺省用默认 spec
      warehouse_default: "SH.001"
      fields_from_session: [level, username, cooperationType, userAttr]
      org_code: <null | 值>              # uat-sea fallback = "JTExpress"
      include_user: true
    required_headers:                    # 值为字面量或 ${session.*} 表达式
```

内置 fallback 必需头：`x-area: hnzy`、`X-DB: ${session.db}`、`X-User: ${session.user}`、`X-Locale: ZH`、`langType: ZH`、`timezone: GMT+0800`（有 x_env 加 `x-env`；uat-cn 额外 `X-Authentication-Switch: true`）。模板头按头名小写去重后覆盖基础头。

## 占位符解析规则

1. 正则 `\{\{\s*([A-Za-z0-9_.]+)\s*\}\}`，作用于 url + 头值 + body，按 canonical 名去重。
2. 来源：`{{process.env.NAME}}` → canonical=NAME；`{{name}}` 有 vars 映射（模板内 vars 块或 `api/bruno/environments/local.bru` 优先）→ 映射的 env 名；无映射 → 启发式 camelCase→`WMS_` 大写下划线（`xPdaVersion`→`WMS_X_PDA_VERSION`）。
3. 求值顺序：用户 vars（canonical 名）→ 用户 vars（env 名）→ 会话/环境派生（WMS_BASE_URL=gateway、WMS_X_TOKEN/CLIENT/USER/DB/X_PARAMS、WMS_WAREHOUSE_CODE、WMS_OWNER_CODE/WMS_COMPANY_CODE=`JMJ001`、WMS_OPERATOR_CODE=session.user、WMS_X_PDA_VERSION=`V1.0.0`）→ 缺失则整体返回 `missing` 不发送。
4. kind 判定：catalog 显式 `kind` 优先；否则词表匹配（query/list/page 等判 test，create/cancel/confirm 等判 create），默认 create。

## 造数能力检索

- **GET `/api/capabilities?project=WMS&target=&domain=&q=`**：过滤参数可选。`target` 匹配 targets[].name/label/statusCode；`q` 对 id/domain/purpose/script/recipe/notes 包含匹配。返回 normalized summary 列表（id/domain/object/purpose/script/recipe/verifiedEnv/runHint/status 等）。
- **GET `/api/capability?id=<id>&project=WMS`**：summary + 原始 capability + `targets` + `cli`（参数白名单）+ `pitfalls` + `notes`。
- **GET `/api/capability/sources`**：pack 挂载状态（`status: ok|missing`）。注意其 notes 文案仍写"read-only"，已过时——实际执行能力见下节。

## 造数执行：POST `/api/testdata/run`

Body（JSON 或表单）：

| 参数 | 必填 | 说明 |
|---|---|---|
| `project` | 是 | 当前固定 `WMS` |
| `capabilityId` | 是 | capability id |
| `target` | 是 | 必须存在于 capability `targets[].name`，否则 400 |
| `env` | 否 | 透传给脚本；白名单 `test`/`uat-cn`/`uat-sea`（其它值 400） |
| `params` | 否 | `Record<string,string>`，只接受 capability `cli` 声明的键，非空时追加 `--key value` |
| `dryRun` | 否 | 默认 true |
| `execute` | 否 | 默认 false |
| `timeoutSecs` | 否 | 执行超时秒数，默认 180，clamp 30~600；dry-run 回显 `timeoutSecs` |

- **dry-run**（`!execute || dryRun`，即默认）：不执行，返回构造好的 `command`（`uv run python <script> --target <target> ...`）与 `args` 预览。
- **执行**（`execute=true && dryRun=false`）：在 pack 根 cwd spawn 脚本，超时默认 180s（`timeoutSecs` 可调，上限 600s）；超时错误会提示批量造数（`--count`）直接跑脚本；返回 `{ok:<exit0>, exitCode, stdout, stderr, command}`；spawn 失败/超时 400。
- 仅支持 `execution == "script"` 的能力。
- 前置检查（脚本自身 UAT 禁写、目标状态校验）仍由 testdata pack 的脚本与 `wms-test-data-creation` 流程保证；用 panel 执行不豁免这些规则。**通道选择**：常规/批量造数默认直接跑脚本（`wms-test-data-creation`），panel run 适合单发造数 + 统一审计/配合面板 UI。

## 注意事项

- `GET /api/apitest/apis` 与详情的 `project` query 参数当前未参与过滤，传了等于没传。
- 降级登录会话（仅顶层 token，无 session）时 `X-User`/`X-DB` 派生为空，依赖它们的模板会报缺占位符——先解决登录态（Chrome 登录 WMS）。
- 工时/任务推荐相关 API 见 `requirement-api.md` 的「ONES 工时联动」节。
