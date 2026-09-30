//! `boa_engine` 包装。**每次调用都新建一个 boa Context**: boa 的 `Context` 不是 `Send`, 多线程下载
//! 场景下不能共享同一个; 而创建 Context 只是分配一组 vec/map, 开销远小于一次 HTTP 请求。

use anyhow::{Context as _, Result};
use boa_engine::vm::RuntimeLimits;
use boa_engine::{Context, Source};
use std::fmt::Write as _;

/// JS 规则执行的资源护栏。
///
/// `eval()` 是同步阻塞, 没有 wall-clock timeout API, 所以靠 `RuntimeLimits` 计数循环 + 递归, 超限
/// 立即 throw `RuntimeError` 而不是卡死主流程: `loop_iteration = 100_000` 挡 `while(true){}` 类恶意
/// 规则 (远大于真实后处理典型 10-1000 步), `recursion_limit = 256` 挡无限递归。console shim 见下。
fn apply_runtime_limits(ctx: &mut Context) {
    let mut limits = RuntimeLimits::default();
    limits.set_loop_iteration_limit(100_000);
    limits.set_recursion_limit(256);
    ctx.set_runtime_limits(limits);
}

/// 注入 no-op `console`: boa 默认没有 `console` 全局对象, 规则里的 `console.log` 会中断整段脚本。
fn install_console_shim(ctx: &mut Context) {
    let _ = ctx.eval(Source::from_bytes(
        r"
        var console = console || {
            log: function(){}, info: function(){}, warn: function(){},
            error: function(){}, debug: function(){}, trace: function(){}
        };
        ",
    ));
}

/// 等价 Java `JsCaller#call`: 把 `input` 当作变量 `r` 注入, 执行 `body`, 再返回 `r`。
///
/// `body` 是规则中 `@js:` 后面的 JS 片段 (不带 `function func()` 包装); 含 `return` 时包装成
/// IIFE, 让 `return` 合法: `(function(r){<body>; return r;})(input)`。
pub fn post_process(body: &str, input: &str) -> Result<String> {
    // 注入 input 时用 JSON.stringify 风格转义，避免引号 / 反斜杠破坏脚本。
    let mut ctx = Context::default();
    apply_runtime_limits(&mut ctx);
    install_console_shim(&mut ctx);
    let injected_input = json_quote_for_js(input);

    let script = if body.contains("return") {
        format!("(function(r){{ {body}; return r; }})({injected_input})")
    } else {
        format!("var r = {injected_input};\n{body};\nr")
    };

    let value = ctx
        .eval(Source::from_bytes(&script))
        .map_err(|e| anyhow::anyhow!("js eval failed: {e}"))?;

    let s = value
        .to_string(&mut ctx)
        .map_err(|e| anyhow::anyhow!("js value to_string failed: {e}"))?;
    s.to_std_string()
        .context("js result is not valid UTF-16 → UTF-8")
}

/// 加载一个 JS 文件 (含全局函数定义), 调用其中某个函数, 返回字符串结果。
/// 等价 Java `JsCaller#callFunction`, 入参/出参都是字符串, 不做 ECMAScript Value 互转。
pub fn eval_function_returning_string(
    module_js: &str,
    fn_name: &str,
    string_args: &[&str],
) -> Result<String> {
    let mut ctx = Context::default();
    apply_runtime_limits(&mut ctx);
    install_console_shim(&mut ctx);

    ctx.eval(Source::from_bytes(module_js))
        .map_err(|e| anyhow::anyhow!("js module eval failed: {e}"))?;

    let args_js: Vec<String> = string_args.iter().map(|a| json_quote_for_js(a)).collect();
    let call = format!("{fn_name}({})", args_js.join(", "));
    let value = ctx
        .eval(Source::from_bytes(&call))
        .map_err(|e| anyhow::anyhow!("js call '{fn_name}' failed: {e}"))?;

    let s = value
        .to_string(&mut ctx)
        .map_err(|e| anyhow::anyhow!("js return to_string failed: {e}"))?;
    s.to_std_string()
        .context("js result is not valid UTF-16 → UTF-8")
}

/// 把字符串包装为 JS 字面量 (最小 JSON-quote 即可, 不用 boa 内部的 `js_string!`)。
fn json_quote_for_js(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str(r#"\""#),
            '\\' => out.push_str(r"\\"),
            '\n' => out.push_str(r"\n"),
            '\r' => out.push_str(r"\r"),
            '\t' => out.push_str(r"\t"),
            '\u{0008}' => out.push_str(r"\b"),
            '\u{000C}' => out.push_str(r"\f"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
    use super::*;
    use std::path::PathBuf;

    fn repo_web() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("bundle")
            .join("web")
    }

    // ---------- post_process（@js: 片段后处理）----------

    #[test]
    fn replace_string_literal() {
        let body = r"r = r.replace('作者：', '')";
        let out = post_process(body, "作者：苹果").unwrap();
        assert_eq!(out, "苹果");
    }

    #[test]
    fn regex_replace_with_function_callback() {
        // main.json 里有：r=r.replace(/\(\d+\/\d+\)/, '');
        let body = r"r = r.replace(/\(\d+\/\d+\)/, '')";
        let out = post_process(body, "第1章 标题(1/3)").unwrap();
        assert_eq!(out, "第1章 标题");
    }

    #[test]
    fn url_concatenation_pattern() {
        // main.json 第三条："coverUrl": "meta[...]@js:r='http://www.mcxs.info'+r"
        let body = r"r = 'http://www.mcxs.info' + r";
        let out = post_process(body, "/cover/123.jpg").unwrap();
        assert_eq!(out, "http://www.mcxs.info/cover/123.jpg");
    }

    #[test]
    fn handles_input_with_quotes_and_newlines() {
        let body = r"r = r.toUpperCase()";
        let input = "say \"hi\"\nnext";
        let out = post_process(body, input).unwrap();
        assert_eq!(out, "SAY \"HI\"\nNEXT");
    }

    #[test]
    fn return_statement_in_body_uses_iife() {
        // 模拟 quanben5 @js: URL：body 含 return，需 IIFE 包装。
        let body = r#"var x = r + "!"; return x"#;
        let out = post_process(body, "hello").unwrap();
        assert_eq!(out, "hello!");
    }

    #[test]
    fn return_url_concatenation_pattern() {
        // 模拟 proxy-required.json quanben5 搜索 URL 的 @js: 表达式
        let body = r"return 'https://example.com/?q=' + r";
        let out = post_process(body, "测试").unwrap();
        assert_eq!(out, "https://example.com/?q=测试");
    }

    #[test]
    fn matchall_es6() {
        // rate-limit.json 实战样例（精简）：用 matchAll 抽取 dd id 后排序拼接
        let body = r#"
            const ddMatches = [...r.matchAll(/<dd\s+data-id="(\d+)">([\s\S]*?)<\/dd>/g)];
            const ddList = ddMatches.map(m => ({id: Number(m[1]), content: m[2]}));
            ddList.sort((a, b) => a.id - b.id);
            r = ddList.map(d => d.content).join('|');
        "#;
        let input = r#"<dd data-id="2">B</dd><dd data-id="1">A</dd>"#;
        let out = post_process(body, input).unwrap();
        assert_eq!(out, "A|B");
    }

    #[test]
    fn infinite_loop_is_bounded_by_runtime_limits() {
        // 验证 RuntimeLimits 生效: while(true) 触发 loop_iteration_limit 抛错, 不会卡死。
        let body = "while(true){}";
        let result = post_process(body, "x");
        assert!(result.is_err(), "infinite loop must error, got {result:?}");
    }

    // ---------- eval_function（独立 JS 文件 + 函数调用）----------

    #[test]
    fn loads_test_resource_js_chapter_module() {
        // 测试资源里的 96dushu JS 是"自带 r 字面量 + 处理"的脚本 (按"复制粘贴到浏览器
        // 调试"的格式); 我们的 post_process 也先注入 var r 再执行, 顺序兼容这类脚本。
        let js_path = repo_web().join("js").join("96dushu-chapter.js");
        let body = std::fs::read_to_string(&js_path).unwrap();

        // 脚本里 r = `<html...>` 自我赋值会覆盖注入的 var r, 只要不抛异常即视为通过。
        let _ = post_process(&body, "ignored").expect("96dushu-chapter.js should run on boa");
    }
}
