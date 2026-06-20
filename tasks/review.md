# Phase 2 质量优化 — 变更复盘

> 时间跨度：单次会话。
> 配套文档：`tasks/audit-summary.md`（审计输入）、`tasks/todo.md`（执行 checklist）。
> 起点：master `196d413`（v0.2.6 bundled changes）。
> 终点：master `bc410c1`（共 +4 commit，全部小步、单一关注点）。

---

## 1. 变更清单

| Commit | 类型 | 模块 | 影响 |
| --- | --- | --- | --- |
| `418f79c` | perf | `db::sources::seed_from_default` | 13 条默认规则 INSERT 包进单一事务：1 次 fsync + 整批原子 |
| `9c10c4c` | perf | `db::tasks::delete_finished` | N+1 round-trip → 单 SQL `DELETE WHERE json_extract(...).finished IS NOT NULL` |
| `ee78f4b` | feat | `main::purge_old_logs` | 启动时清 `~/.sonovel/logs/` 里 mtime > 30d 的文件 |
| `bc410c1` | fix | `app::runtime::build_shared_runtime` | tokio builder 失败改 `Result`，不再 panic |

另有 5 处 Phase 2.1.x 防御性 expect/panic 修复（panics DB init、semaphore × 2、retry、zhconv），已在 audit 阶段一并纳入 commit `196d413` 的前身（`v0.2.6` bundled）之前由前面几个会话完成，这里不再单列。

---

## 2. 验证结果

```text
cargo build --all-targets --all-features     ✓ 0 errors
cargo clippy --all-targets --all-features -- -D warnings   ✓ 0 warnings
cargo test --all-targets --all-features      ✓ 292 passed (was 284, +8)
```

Phase 2 新增的 8 个测试：

| 模块 | 测试名 | 覆盖点 |
| --- | --- | --- |
| `config::loader` | `save_config_overwrites_existing_without_leaving_tmp` | 原子写不残留 tmp 文件 |
| `config::loader` | `save_config_writes_to_new_path` | 首次写入新路径同样原子 |
| `db::tasks` | `delete_finished_only_finished` | 混合数据下只删 finished |
| `db::tasks` | `delete_finished_empty_or_all_running` | 边界：空表 / 全 running 不报错 |
| `db::tasks` | `delete_finished_idempotent` | 二次调用返 0 |
| `main` | `purge_old_logs_removes_only_expired` | 31 天前的删，1 天前的留 |
| `main` | `purge_old_logs_skips_subdirectories` | 子目录不被波及 |
| `main` | `purge_old_logs_zero_retention_removes_all` | retention=0 → 全删 |

---

## 3. 兼容性影响

- **CLI 子命令 / 输出 schema** — 无变更。
- **`~/.sonovel/` 数据目录** — 无 schema 迁移。`seed_from_default` 仍然只在 `sources` 表为空时执行；`delete_finished` 仍只删 `finished IS NOT NULL` 行。
- **`config.toml`** — 仍然向后兼容读老格式（`toml_edit` 解析）。原子写只改变"写入路径"的安全性，不改解析。
- **日志文件名** — 仍然是 `log_dir/YYYY-MM-DD`（空 prefix）；只是多了"30 天前的会被删"的副作用。
- **性能** — `seed_from_default` 13 条规则场景下从 13×fsync 降到 1×fsync（量级小到用户无感，但是是 transactional safety 的核心）；`delete_finished` 100 条混合数据下从 101 round-trip 降到 1 round-trip（典型任务列表 < 50 条时仍只是线性变快，不是 hot path）。
- **DB re-export** — `src/db/mod.rs` 现在 `pub use` 出 `FinishedReason`，是 additive 变更（之前通过 `db::tasks::FinishedReason` 仍可用）。

---

## 4. 残留风险

| 风险 | 描述 | 缓解 |
| --- | --- | --- |
| Atomic write 在 Windows 上不是 POSIX atomic | `fs::rename` 在 Windows 上要求 target 不存在；现实现是 "remove target → rename tmp → target"，窗口期极短（毫秒级）但理论上不是 single atomic op。 | 两次失败都尝试清理 tmp 文件；下次启动 `load_config` 仍能读旧文件。如果半截写发生在 remove 之后 / rename 之前，下次启动拿到旧 config（按设计："要么旧要么新，无半截"） |
| `purge_old_logs` 在 GUI 模式下静默失败 | `eprintln!` 在 Windows GUI subsystem 下不可见。 | 用户实际能感知的是磁盘占用不再涨；若想看，可走 CLI 子命令 + `RUST_LOG=info`。当前实现不引入额外风险。 |
| `seed_from_default` 错误信息偏技术 | 13 条规则中有 1 条 JSON 序列化失败时，UI 会看到"seed 第 N 条规则失败"。 | 已经用 `with_context` 包装；用户路径上的根因（disk full / permission）通常更靠前 |

---

## 5. 未来建议

Phase 3+5 是下一轮 PR 范围，本 review 只列不动：

- **Phase 3.1 HTTP 客户端复用**：最大收益（按 audit 测算，100 章抓取可省 30-50% 时间）。先做这一项再做其它。
- **Phase 3.6 Crawler cancel 立即响应**：50ms poll → `tokio::sync::Notify`。这个对用户体验（点取消立即停止）影响最直接。
- **Phase 4.1 补测试**：当前 292 测试覆盖 ~70% 模块；`audit-summary.md` §3 列的 ~12 个 gap 优先级：DB 边界 > Parser 边界 > Export 边界。
- **Phase 5 README 同步**：仅在 Phase 3 完成后再统一更新。

不建议在 Phase 2 范围里做的（防止 over-engineering）：

- 引入 `criterion` / `dhat`：基准测试等 Phase 3 性能改完再做才有对照。
- `tokio::sync::RwLock<AppConfig>`：当前 AppModel 单线程独占语义清晰，加锁收益小、复杂度上升。
- 重写中文文档：本轮零对外文案改动，README 不需要动。