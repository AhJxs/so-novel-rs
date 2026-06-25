# 书源集说明

本文档对应 `bundle/rules/` 下的书源文件。**书源规则文件均位于
`bundle/rules/xx.json`**（首次运行会复制到 `~/.sonovel/rules/`）。

| 书源文件 | 用途 | 数量 |
|---|---|---|
| `main.json` | 默认书源，均支持搜索、大陆 IP | 12 |
| `proxy-required.json` | 需要代理的书源（必须是非大陆 IP），需在 `config.toml` 设置 `cf-bypass` | 4 |
| `rate-limit.json` | 下载限流的书源 | 4 |
| `no-search.json` | 不支持搜索的书源，需要输入书籍详情页地址下载 | 2 |
| `cloudflare.json` | 有 Cloudflare 保护的书源，需在 `config.toml` 设置 `cf-bypass` | 3 |
| `rule-template.json5` | 书源规则模板文件（自定义书源参考） | — |

> **⚠️ IP 要求仅供参考，不保证完全准确**。根据需要决定是否在
> `config.toml` 中设置 HTTP 代理（TUN 模式、路由级代理无需设置）。

---

## `main.json`：默认书源

均支持搜索、大陆 IP。

| 书源名称 | 大陆 IP | 非大陆 IP | ⚠️ 需要注意 |
|---|---|---|---|
| [香书小说](http://www.xbiqugu.la/) | ✅ | ❌ | |
| [书海阁小说网](https://www.shuhaige.net/) | ✅ | ✅ | 搜索限流 |
| [梦书中文](http://www.mcxs.info/) | ✅ | ❌ | 搜索限流 |
| [鸟书网](http://www.99xs.info/) | ✅ | ❌ | 搜索限流 |
| [笔趣阁22](https://www.22biqu.com/) | ✅ | ✅ | |
| [笔尖中文](http://www.xbiquzw.net/) | ✅ | ❌ | |
| [书林文学](http://www.shu009.com/) | ✅ | ✅ | 源站目录有重复、缺章的情况。目录每页只有 20 章，翻页速度很慢 |
| [悠久小说网](http://www.ujxsw.org/) | ✅ | ❌ | |
| [阅读库](http://www.yeudusk.com/) | ✅ | ❌ | |
| [顶点小说](https://www.wxsy.net/) | ✅ | ❌ | 搜索、详情限流 |
| [笔趣阁365](https://www.biquge365.net/) | ✅ | ✅ | 搜索间隔 15 秒 |
| [燃文小说网](https://www.ranwen8.cc/) | ✅ | ❌ | |

---

## `proxy-required.json`：需要代理的书源

**必须是非大陆 IP**，需在 `config.toml` 设置 `cf-bypass`（指向你的反代服务，见
[§ 接入 Cloudflare 保护的书源](#接入-cloudflare-保护的书源)）。

| 书源名称 | 支持搜索 | ⚠️ 需要注意 |
|---|---|---|
| [69书吧](https://www.69shuba.com/) | ❌ | 章节页有 CF，推荐线程数 ≤ 5，若绕过失败则提示正文内容为空 |
| [全本小说网](https://quanben5.com/) | ✅ | 完本很全，连载基本搜不到；同 quanben5.io / big5.quanben5.com / quanben-xiaoshuo.com |
| [大熊猫文学](https://www.dxmwx.org/) | ✅ | |
| [101看书](https://101kks.com/) | ✅ | 章节页有 CF，推荐线程数 ≤ 5，UI 同 69 |

---

## `rate-limit.json`：下载限流的书源

| 书源名称 | 大陆 IP | 非大陆 IP | 支持搜索 | ⚠️ 需要注意 |
|---|---|---|---|---|
| [新天禧小说](https://www.tianxibook.com/) | ✅ | ❌ | ✅ | 下载过快会导致章节内容为空，建议线程数 ≤ 5 |
| [零点小说](https://www.0xs.net/) | ✅ | ✅ | ✅ | 限流程度和 69 相似，爬取过快会封 IP 且获取不到正文 |
| [老幺小说网](https://www.laoyaoxs.org/) | ✅ | ❌ | ✅ | 章节限流，正文段落会乱序 |
| [速读谷](https://www.sudugu.org/) | ✅ | ✅ | ✅ | 章节限流，线程数 1 |

---

## `no-search.json`：不支持搜索的书源

需要输入**书籍详情页的 URL** 下载（不能用关键词搜）。

| 书源名称 | 大陆 IP | 非大陆 IP | ⚠️ 需要注意 |
|---|---|---|---|
| [天天看小说](https://cn.ttkan.co/) | ✅ | ✅ | 曾经可以搜索 |
| [小说虎](https://www.xshbook.com/) | ✅ | ✅ | 正文广告较多，需手动过滤 |

---

## `cloudflare.json`：有 Cloudflare 保护的书源

需在 `config.toml` 设置 `cf-bypass` 指向 CF 绕过服务（详见下文
[§ 接入 Cloudflare 保护的书源](#接入-cloudflare-保护的书源)）。

| 书源名称 | 大陆 IP | 非大陆 IP | ⚠️ 需要注意 |
|---|---|---|---|
| [黄易天地](http://www.xhytd.com/) | ✅ | ✅ | 非大陆 IP 可能速度较慢 |
| [96读书](https://www.96dushu.com/) | ✅ | ✅ | 章节 JS 加密 |
| [东滩小说](http://www.dongtanxs.com/) | ✅ | ✅ | |

---

## 接入 Cloudflare 保护的书源

`proxy-required.json` 和 `cloudflare.json` 里的书源都接了 Cloudflare
防护，直接用 `reqwest` 访问会拿到 403 / "Just a moment..." 挑战页。
要绕过，**推荐用 [CloudflareBypassForScraping](https://github.com/sarperavci/CloudflareBypassForScraping)**
跑一个反代服务，再让 `so-novel-rs` 把所有对 CF 站的请求走那个反代。

---

## 若书源无法使用，请参考以下步骤排查

1. **IP 要求**：许多书源对 IP 有要求，确保你的 IP 符合要求（见上文表格）
2. **IP 被封禁**：部分书源（永久）封禁了某些 IP，常见原因：
   - 搜索频率过快
   - 下载间隔过小
   - 下载线程数过大

   解决：换 IP / 等几天 / 调低频率
3. **临时不可用**：一些书源在某些时段可能无法访问（维护、被攻击、
   数据同步），建议多次重试、换个时间再试
4. **爬取参数**：检查下载线程数 / 章节间隔是否合理，否则会被部分
   书源封 IP 或限流
5. **书源挂了**：如果以上都不行，大概率是书源本身挂了（更换域名、
   增加云防护等）。关注 GitHub Issues / 书源仓库的更新

---

## 切换书源集

`bundle/rules/` 下的 5 个书源 JSON 都是**可选的活跃文件**。切换有 3 种方式：

### 方式 1：GUI 书源管理页（推荐）

GUI 顶栏 → **书源** 标签 → 右上角下拉菜单切换活跃文件。变更立即生效，
不需要重启。

> 实现：[`src/app/ops/sources.rs::switch_active_file`](../src/app/ops/sources.rs)
> 改 `SourcesConfig.active_file` 后调 `load_active_rules` 重新加载。

### 方式 2：手动改 `~/.sonovel/sources_config.json`

```json
{
  "active_file": "main.json",
  "disabled_urls": []
}
```

把 `active_file` 改成目标文件名（必须是 `~/.sonovel/rules/` 下已存在的 JSON），
保存后**重启 `so-novel-rs`** 生效。

```sh
# 例：临时切到 proxy-required.json（要确保 config.toml 已配 cf-bypass）
sed -i 's/"active_file": "main.json"/"active_file": "proxy-required.json"/' \
  ~/.sonovel/sources_config.json
```

> 这个文件 GUI / Web / CLI **三处共享写**（都用
> [`SourcesConfig::save`](../src/persistent/sources_config.rs) 原子写）。
> 同一时刻别两个进程同时写，会 last-write-wins。

### 方式 3：复制新书源文件到 `~/.sonovel/rules/`

GUI / Web UI 都支持从文件导入书源（"添加"按钮选 JSON 文件），导入后自动
出现在活跃文件下拉里。

> ⚠️ **注意**：`config.toml` 里**没有** `active-rules` 字段 —— Java 时代的
> 配置项，Rust 端没实现。`SourcesConfig.active_file` 才是唯一的 source of
> truth。任何"通过 `config.toml` 切换书源集"的教程都过时了。

---

## 自定义书源

参考：
- [`bundle/rules/rule-template.json5`](../bundle/rules/rule-template.json5) — 模板文件，含字段说明
- [`bundle/rules/main.json`](../bundle/rules/main.json) — 实际书源集，看真实例子

支持语法：**css selector** / **xpath** / **javascript** / **regex**。
书源格式由 [`src/models/rule.rs`](../src/models/rule.rs) 的 `Rule` struct 定义。

修改后重启 `so-novel-rs` 生效（或 GUI 顶栏的"重新加载书源"按钮）。

---

## 进一步阅读

- [README.md](../README.md) — 项目总览
- [CLI.md](./CLI.md) — CLI 用法
- [WEB.md](./WEB.md) — Web / Docker 部署
- [CHANGELOG.md](./CHANGELOG.md) — 最新 release
- 书源规则定义：[`src/models/rule.rs`](../src/models/rule.rs)
- 书源持久化（`disabled_urls`）：[`src/persistent/sources_config.rs`](../src/persistent/sources_config.rs)
- [CloudflareBypassForScraping](https://github.com/sarperavci/CloudflareBypassForScraping) — CF 绕过服务
