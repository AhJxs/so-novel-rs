//! boa_engine 包装。
//!
//! Java 端 `JsCaller` 用 ThreadLocal 维护一个 V8Runtime，反复执行
//! `function func(r){<body>; return r;}` 然后 `func(input)`。
//!
//! Rust 这里**每次调用都新建一个 boa Context**：boa 的 Context 不是 Send，
//! 而我们将来在多线程下载场景下不能共享同一个；boa Context 创建只是分配
//! 一组 vec/map，开销远小于一次 HTTP 请求，是可接受的代价。
//!
//! 如果将来发现 JS 是热点，再做 ThreadLocal pool。

use anyhow::{Context as _, Result};
use boa_engine::{Context, Source};

/// 给 boa Context 注入一个 no-op `console`，让规则 / 测试资源里的
/// `console.log` 不会把整段脚本中断（boa 默认没有 `console` 全局对象）。
fn install_console_shim(ctx: &mut Context) {
    let _ = ctx.eval(Source::from_bytes(
        r#"
        var console = console || {
            log: function(){}, info: function(){}, warn: function(){},
            error: function(){}, debug: function(){}, trace: function(){}
        };
        "#,
    ));
}

/// 等价 Java `JsCaller#call`：把 `input` 当作变量 `r` 注入，执行 `body`，再返回 `r`。
///
/// `body` 通常是规则中 `@js:` 后面的 JS 片段（不带 `function func()` 包装）。
///
/// 示例：
/// ```ignore
/// post_process("r=r.replace('作者：','')", "作者：苹果").unwrap() == "苹果";
/// ```
pub fn post_process(body: &str, input: &str) -> Result<String> {
    // 包装为：
    //   var r = <input 字面量>;
    //   <body>;
    //   r
    // 注入 input 时用 JSON.stringify，避免输入里的引号 / 反斜杠破坏脚本。
    let mut ctx = Context::default();
    install_console_shim(&mut ctx);
    let injected_input = json_quote_for_js(input);
    let script = format!("var r = {injected_input};\n{body};\nr");
    let value = ctx
        .eval(Source::from_bytes(&script))
        .map_err(|e| anyhow::anyhow!("js eval failed: {e}"))?;

    let s = value
        .to_string(&mut ctx)
        .map_err(|e| anyhow::anyhow!("js value to_string failed: {e}"))?;
    s.to_std_string()
        .context("js result is not valid UTF-16 → UTF-8")
}

/// 加载一个 JS 文件（含全局函数定义），调用其中某个函数，返回字符串结果。
///
/// 等价 Java `JsCaller#callFunction`，但当前只用于 quanben5 search 加密参数 `b`，
/// 入参/出参都是字符串，不需要全 ECMAScript Value 互转。
pub fn eval_function_returning_string(
    module_js: &str,
    fn_name: &str,
    string_args: &[&str],
) -> Result<String> {
    let mut ctx = Context::default();
    install_console_shim(&mut ctx);

    // 1. 先把 module 的全局函数装进 context
    ctx.eval(Source::from_bytes(module_js))
        .map_err(|e| anyhow::anyhow!("js module eval failed: {e}"))?;

    // 2. 构造 `fn_name("a", "b", ...)` 并执行
    let args_js: Vec<String> = string_args.iter().map(|a| json_quote_for_js(a)).collect();
    let call = format!("{fn_name}({})", args_js.join(", "));
    let value = ctx
        .eval(Source::from_bytes(&call))
        .map_err(|e| anyhow::anyhow!("js call '{fn_name}' failed: {e}"))?;

    // boa 不会因 console.log 失败；规则 JS 里的 `console.log` 调用会被自然丢弃。
    let s = value
        .to_string(&mut ctx)
        .map_err(|e| anyhow::anyhow!("js return to_string failed: {e}"))?;
    s.to_std_string()
        .context("js result is not valid UTF-16 → UTF-8")
}

/// 把字符串包装为 JS 字面量。优先 JSON.stringify 风格；
/// boa 已有 `js_string!` 宏但只用于内部 JsString；这里需要拼接成源码，
/// 直接做最小的 JSON-quote 即可。
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
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
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
        let body = r#"r = r.replace('作者：', '')"#;
        let out = post_process(body, "作者：苹果").unwrap();
        assert_eq!(out, "苹果");
    }

    #[test]
    fn regex_replace_with_function_callback() {
        // main.json 里有：r=r.replace(/\(\d+\/\d+\)/, '');
        let body = r#"r = r.replace(/\(\d+\/\d+\)/, '')"#;
        let out = post_process(body, "第1章 标题(1/3)").unwrap();
        assert_eq!(out, "第1章 标题");
    }

    #[test]
    fn url_concatenation_pattern() {
        // main.json 第三条："coverUrl": "meta[...]@js:r='http://www.mcxs.info'+r"
        let body = r#"r = 'http://www.mcxs.info' + r"#;
        let out = post_process(body, "/cover/123.jpg").unwrap();
        assert_eq!(out, "http://www.mcxs.info/cover/123.jpg");
    }

    #[test]
    fn handles_input_with_quotes_and_newlines() {
        let body = r#"r = r.toUpperCase()"#;
        let input = "say \"hi\"\nnext";
        let out = post_process(body, input).unwrap();
        assert_eq!(out, "SAY \"HI\"\nNEXT");
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

    // ---------- eval_function（独立 JS 文件 + 函数调用）----------

    #[test]
    fn quanben5_get_param_b_returns_non_empty() {
        //   getParamB(keyword) -> 加密后的字符串
        //
        // 算法：encodeURI(keyword) → 每字符在 staticchars(62) 内做 +3 偏移，
        // 不在表内则保留；外层每输出 1 个原字符插入 2 个随机字符（共 3）；
        // 最后再 encodeURI 一次（让 `%` 也被编码为 `%25`）。
        // 因此长度并非简单 keyword.len()*3 — 不要硬编码长度，只断言可重复且非空。
        let js_path = repo_web().join("js").join("quanben5.js");
        assert!(js_path.exists(), "missing {}", js_path.display());

        let module = std::fs::read_to_string(&js_path).unwrap();
        let out = eval_function_returning_string(&module, "getParamB", &["三体"]).unwrap();
        assert!(!out.is_empty(), "param b should not be empty");
        // 输出必须是纯 ASCII（所有非 ASCII 字符都被 encodeURI 转成 %XX）
        assert!(
            out.bytes().all(|b| b < 128),
            "expected ASCII-only output, got {out:?}"
        );
    }

    #[test]
    fn quanben5_param_b_uses_only_static_chars_and_uri_chars() {
        // 验证 JS 行为细节：staticchars + URI 编码后的 % 数字会一并输出。
        // 这里只断言结果中没有非 ASCII 字节，因为 base64 函数把任何字符
        // 都映射到 staticchars 里的 ASCII 字符；URI 编码后是 % + hex。
        let js_path = repo_web().join("js").join("quanben5.js");
        let module = std::fs::read_to_string(&js_path).unwrap();
        let out = eval_function_returning_string(&module, "getParamB", &["abc"]).unwrap();
        assert!(
            out.bytes().all(|b| b < 128),
            "expected ASCII-only output, got {out:?}"
        );
    }

    #[test]
    fn loads_test_resource_js_chapter_module() {
        // 测试资源里的 96dushu / wxsy.net JS 都是"自带 r 字面量 + 处理"
        // 的脚本（按"复制粘贴到浏览器调试"的格式）。Java 端运行环境
        // 通过 `JsCaller.JS_TEMPLATE` 已经把 r 作为函数参数注入。
        // 我们的 post_process 也是先注入 var r= 再执行，因此这里
        // 用 post_process 的逻辑、给一个空 input，验证 boa 能跑这种脚本。
        let js_path = repo_web().join("js").join("96dushu-chapter.js");
        let body = std::fs::read_to_string(&js_path).unwrap();

        // 直接通过 post_process 运行：内部会先注入 var r 再执行。
        // 96dushu 脚本里有 r = `<html...>` 自我赋值，覆盖了我们的 var r，
        // 也能跑通——只要不抛异常即视为通过。
        let _ = post_process(&body, "ignored").expect("96dushu-chapter.js should run on boa");
    }

    #[test]
    fn quanben5_search_js_runs() {
        let js_path = repo_web()
            .join("js")
            .join("quanben5.com-search.js");
        let module = std::fs::read_to_string(&js_path).unwrap();
        // 这个脚本只定义函数，没有顶层副作用，eval 通过即可。
        let out = eval_function_returning_string(&module, "getParamB", &["三体"]).unwrap();
        assert!(!out.is_empty());
        assert!(out.bytes().all(|b| b < 128));
    }
}
