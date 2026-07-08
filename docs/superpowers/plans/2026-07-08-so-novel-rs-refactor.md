# so-novel-rs 企业级优化改造计划

> 实施日期: 2026-07-08
> 适用项目: so-novel-rs (Rust 桌面客户端, GUI/CLI/Web 三端)
> 目标: 达成企业级工程标准, 不颠覆业务, 不改对外接口

---

## 〇、用户确认决策 (6 项)

1. ✅ `util/` → `utils/` (改名)
2. ✅ `persistent/` → `db/` (改名)
3. ✅ clippy pedantic 后续可清除即可接受
4. ✅ AppError 逐模块迁移 (不一次性)
5. ✅ 核心逻辑在 `service/` 测, GUI 页面层只 smoke
6. ✅ README 重写在模块改造完成后

---

## 一、现状摸底 (决定取舍)

### 已落地, 不重做
- 分层目录基本符合提示词: `config/`、`models/`、`util/`、`persistent/`、`parser/`、`crawler/`、`cli/`/`web/`/`gpui_app/`、`export/`
- 错误体系: 已用 `thiserror` 定义多个领域级枚举 (`ExportError`、`WebError` 及 5 个子域 `BookError/TocError/ChapterError/SearchError/CrawlerError`),`WebError` 用嵌套聚合 + `classify()` 分类为 HTTP 状态
- 日志: `tracing` + `tracing-subscriber`, `TraceId` 串联事件
- 配置: `AppConfig` 结构化 + `toml_edit` (保注释) + `with_defaults()` 兜底
- `rustfmt.toml` 已配: `max_width=100`, `use_field_init_shorthand`, `use_try_shorthand`
- 测试: `cli/tests.rs`、`config/tests.rs`、`web/tests.rs` 已有集成测试骨架
- `Cargo.toml` 已有 `features` (`gui`/`web`) 分层

### 真实差距 (聚焦的)
1. ❌ 没有根 `AppError` — 业务层混用 `anyhow` + 多个 `thiserror` 枚举, 跨域转换靠 `map_err` 字符串拼接
2. ❌ 没有 `constant/` 目录 — UA 池、主题色范围、限流阈值等魔法值散落
3. ❌ 没有 `.clippy.toml` — clippy 全用默认值
4. ❌ `app/` 模块过胖 — 23 个文件, 需轻量子目录分组
5. ❌ 测试覆盖不均 — `crawler/`、`export/`、`http/` 缺单元测试
6. ❌ `middleware/` 缺失 — Web 路由层限流分散
7. ⚠️ `util/` 工具分类偏粗 — 需按提示词细分
8. ⚠️ `dao/` 命名缺失 — 现有 `persistent/` 实际承担 dao 职责
9. ⚠️ service/handler 边界未明 — `crawler/` 兼具编排+实现
10. ⚠️ README 缺企业级部署/排障章节

---

## 二、第一阶段: 整体框架工程管理优化

### 2.1 目录重命名 + Cargo.toml lint
- `git mv src/util src/utils`
- `git mv src/persistent src/db`
- 更新 `lib.rs` `pub mod` 声明
- 全仓替换 `use crate::util::` → `use crate::utils::`
- 全仓替换 `use crate::persistent::` → `use crate::db::`
- `Cargo.toml` 加 `[lints.clippy]` + `[lints.rust]`
- 新建 `.clippy.toml`
- 增强 `.rustfmt.toml` (imports_granularity、group_imports 等)
- `lib.rs` 顶部加 `#![warn(missing_docs)]`

### 2.2 全局基础能力统一
- 错误体系: 新增 `src/error/mod.rs` (根 `AppError` + `AppResult`), 各领域错误保留, 通过 `From` 归一
- 日志: 文档化日志级别矩阵, 已用 `tracing` 框架不重做
- 新建 `src/constant/`: 5 个子模块 (http/theme/limits/paths/format + error_code)
- `utils/` 子模块按 time/string/fs/encoding/validation/rand/convert 细分
- 工具去重: `format_timestamp`、byte→human readable 等统一

### 2.3 全局代码规范
- 命名: 已 Rust 官方, 补 `docs/style-guide.md`
- 格式: rustfmt 已配, CI 加 `cargo fmt --check`
- 注释: 文档化"必须加 doc comment 的清单"
- 语法: clippy pedantic 触发清理
- 安全: `unsafe_code = "forbid"` 锁定

---

## 三、第二阶段: 分模块改造

### 模块 1: `config/` 配置模块
- 拆分 `AppConfig` 为 6 个 sub-struct (GlobalCfg / DownloadCfg / SourceCfg / CrawlCfg / CookieCfg / ProxyCfg)
- 新增 `ConfigError` 强类型错误
- 启动期 `validate(&self)` 校验 (port 范围、font_size ∈ [12,24]、min_interval < max_interval)
- `LazyLock<AppConfig>` 全局单例
- 补 tests.rs 边界用例

### 模块 2: `constant/` 常量枚举
- 扫描全仓魔法值归集
- 关键枚举: `ExportFormat` / `Language` / `ThemePref` / `LogLevel` / `CookieSite`
- 所有枚举 `Display + FromStr + Serialize + Deserialize`
- 错误码常量表 (1xxx 系统/2xxx 业务/3xxx 参数/4xxx 权限)
- `web/error.rs` 文案统一改 `ErrorCode::*.as_str()`

### 模块 3: `error/` 错误处理
- 新增 `src/error/mod.rs` 根 `AppError` + `AppResult<T>`
- 新增 `src/error/convert.rs` 统一 `From`
- 现状对齐: `ExportError`/`WebError` 保留, 加 `From<...> for AppError`
- 消灭 `Result<T, String>`: grep 全仓, 预计 ~30 处
- 消灭裸 `unwrap()/expect()` 在非测试代码
- 错误日志分级映射

### 模块 4: `utils/` 工具模块
- `git mv` + 拆 8 个子模块 (time/string/fs/encoding/validation/rand/convert/lock)
- 合并重复函数
- doctest + 单元测试, 覆盖率 ≥ 80%

### 模块 5: `model/` 数据模型
- DTO/Param 拆分: `book_dto.rs` / `book_param.rs` / `chapter_dto.rs` / `search_dto.rs`
- 字段 `#[serde(rename = "...")]` 与 web-ui 对齐
- 显式 `From<Po> for Dto`, 禁止 Dto 复用 Po
- 字段合法性校验: `impl Book { pub fn validate(&self) }`
- 清理已废弃字段

### 模块 6: `db/` 数据访问层 (原 persistent)
- `git mv` + 加 `DaoError` 强类型
- 收敛 IO 走 `tokio::fs`
- 关键操作 `tracing::instrument` 标注
- 单元测试覆盖 CRUD

### 模块 7: `service/` 业务服务层 (新增)
- 从 `crawler/` 抽出:
  - `service/download.rs` 编排 (TOC + 章节并发 + 断点续传 + 导出)
  - `service/search.rs` 编排 (多源聚合 + 相似度去重 + 排序)
  - `service/cover.rs` 封面编排
- `crawler/` 降级为基础设施层
- service 不直接 I/O / 不直接解析
- ASCII 流程图注释
- 单元测试覆盖率 ≥ 70%

### 模块 8: `handler/` 入口处理层
- 目标: 接参 → 校验 → 调 service → 装响应
- `web/handlers/` 5 文件 + `cli/` 4 子命令 + `gpui_app/pages/` 5 页面, 逐个剥离业务到 service
- 统一 `ApiResponse<T>` 结构
- 入口 `tracing::info_span` 耗时统计
- 废弃入口清理

### 模块 9: `middleware/` 中间件 (新增, Web 专属)
- `cors.rs` / `trace.rs` / `rate_limit.rs` / `recover.rs` / `auth.rs`
- `#[cfg(feature = "web")]` 隔离
- 中间件顺序文档化: `Trace → Recover → RateLimit → CORS → 业务`

### 模块 10: 测试体系
- 单元测试补齐: `crawler/{retry,search,health}`、`export/{epub,txt,html,pdf}`、`http/{fetch,encoding,cf}`、`utils/`、`error/`
- 集成测试: `tests/{download_e2e, search_aggregate, config_roundtrip, web_api}.rs`
- 命名规范: `test_<unit>_<scenario>_<expected>`
- 性能基准: `benches/{download, search}.rs`
- CI: `cargo fmt --check` + `cargo clippy -D warnings` + `cargo test`

---

## 四、收尾验收 (12 项)

| # | 验收项 | 命令/证据 | 通过标准 |
|---|---|---|---|
| 1 | 编译通过 | `cargo check --all-features` | exit 0 |
| 2 | 主告警清零 | `cargo build --all-targets` | 0 warning |
| 3 | clippy 严格通过 | `cargo clippy --all-targets --all-features -- -D warnings` | exit 0 |
| 4 | 格式统一 | `cargo fmt --all -- --check` | exit 0 |
| 5 | 测试通过 | `cargo test --all-features` | 0 failed |
| 6 | 覆盖率 | `cargo tarpaulin` | 整体 ≥ 60%, 核心 ≥ 75% |
| 7 | 文档完整 | `cargo doc --no-deps --all-features` | 0 warning |
| 8 | 无 unsafe | `rg "unsafe" src/` | 0 命中 |
| 9 | 无 unwrap | `rg "\.unwrap\(\)\|\.expect\(" src/` | 仅测试/豁免 |
| 10 | 无 String 错误 | `rg "Result<.*String>" src/` | 0 命中 |
| 11 | 无 println | `rg "println!\|eprintln!" src/` | 0 命中 |
| 12 | 依赖审计 | `cargo audit` | 0 high/critical |

---

## 五、明确不做 (避免过度工程)

1. ❌ 不给 CLI/GUI 引入 axum 中间件链 (middleware 仅 `#[cfg(feature = "web")]`)
2. ❌ 不强行套 `Repository<T, ID>` trait (当前 1 个后端, 不抽)
3. ❌ 不引入 actix/rocket (axum 已 OK)
4. ❌ 不写 plugin 框架 (js/ 已是 plugin)
5. ❌ 不引入配置中心 (桌面应用无此需求)
6. ❌ 不全替换 anyhow (main.rs 和测试 setup 保留)
7. ❌ 不重写 `crawler/` 全部 (只抽"编排"到 service/)
8. ❌ 不强制 100% 覆盖率 (60% 整体 / 75% 核心 即可)

---

## 六、PR 编排 (16 个 PR)

| PR | 工作量 | 内容 | 风险 |
|---|---|---|---|
| #1 | 0.5d | 目录重命名 + lint 配置 | 极低 |
| #2 | 1.5d | AppError 根 + 公共 From | 中 |
| #3 | 0.5d | constant/ 新建 | 低 |
| #4 | 1d | utils 子模块细分 | 低 |
| #5 | 1d | clippy pedantic 整改 | 中 |
| #6 | 1d | config 拆分 + 校验 | 低-中 |
| #7-9 | 2d | AppError 逐模块迁移 (3 批) | 中 |
| #10 | 0.5d | model DTO 拆分 | 低 |
| #11 | 1d | db/ 职责收敛 | 低 |
| #12 | 1.5d | service/ 抽出 | 中-高 |
| #13 | 1d | handler 精简 | 中 |
| #14 | 0.5d | middleware 新建 | 低 |
| #15 | 1.5d | 测试补齐 | 低 |
| #16 | 0.5d | README + 收尾 | 低 |
| **合计** | **~13.5d** | | |

每 PR 控制 ≤ 500 行 diff, 不 commit (除非用户要求)。

---

## 七、当前进度

- [x] 计划已确认 (6 决策已收齐)
- [ ] PR #1: 目录重命名 + lint 配置 ← 当前
- [ ] PR #2-16: 见上表
