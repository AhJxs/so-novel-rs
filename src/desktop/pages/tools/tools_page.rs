//! 工具箱：格式转换 / 编码转换 / 简繁转换。
//!
//! 左侧工具导航 + 右侧表单；三种工具共享「源文件 / 输出目录 / 运行 / 结果」骨架，
//! 只有选项区不同。任务在 `background_executor` 上跑，避免阻塞 UI。

use std::path::{Path, PathBuf};

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{
    ActiveTheme as _, Disableable, Icon, Selectable, Sizable, h_flex, v_flex,
};
use gpui_kit::{
    AppContext, Context, Entity, IntoElement, ParentElement, Render, SharedString, Styled, Window,
    div, px,
};
use rust_i18n::t;

use crate::config::{ExportFormat, LangType};
use crate::desktop::components::PageHeader;
use crate::desktop::model::AppModel;
use crate::tools::{ToolRequest, detect_file_encoding, run};

/// 支持的源文件扩展名（文件选择器过滤用）。
const SOURCE_EXTENSIONS: &[&str] = &["txt", "md", "markdown", "html", "htm", "epub", "zip"];

/// 可选的源编码；`auto` 表示自动探测。
const SOURCE_ENCODINGS: &[&str] = &["auto", "UTF-8", "GBK", "GB18030", "Big5", "Big5-HKSCS"];

/// 可选的输出编码；不含 UTF-16（`encoding_rs` 的 UTF-16 只支持解码）。
const TARGET_ENCODINGS: &[&str] = &["UTF-8", "GBK", "GB18030", "Big5", "Big5-HKSCS"];

/// 三个工具。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToolKind {
    Convert,
    Encoding,
    Zhconv,
}

impl ToolKind {
    const ALL: [Self; 3] = [Self::Convert, Self::Encoding, Self::Zhconv];

    const fn index(self) -> usize {
        match self {
            Self::Convert => 0,
            Self::Encoding => 1,
            Self::Zhconv => 2,
        }
    }

    const fn label_key(self) -> &'static str {
        match self {
            Self::Convert => "Tools.kind.convert",
            Self::Encoding => "Tools.kind.encoding",
            Self::Zhconv => "Tools.kind.zhconv",
        }
    }

    const fn description_key(self) -> &'static str {
        match self {
            Self::Convert => "Tools.desc.convert",
            Self::Encoding => "Tools.desc.encoding",
            Self::Zhconv => "Tools.desc.zhconv",
        }
    }

    const fn icon(self) -> IconName {
        match self {
            Self::Convert => IconName::ArrowRightLeft,
            Self::Encoding => IconName::FileOutput,
            Self::Zhconv => IconName::Languages,
        }
    }
}

/// 单个工具的表单状态。
#[derive(Debug, Clone)]
struct ToolForm {
    source: Option<PathBuf>,
    out_dir: Option<PathBuf>,
    detected: Option<SharedString>,
    target_format: ExportFormat,
    txt_encoding: String,
    source_encoding: String,
    target_encoding: String,
    zhconv_target: LangType,
    running: bool,
    result: Option<PathBuf>,
    error: Option<(SharedString, String)>,
}

impl Default for ToolForm {
    fn default() -> Self {
        Self {
            source: None,
            out_dir: None,
            detected: None,
            target_format: ExportFormat::default(),
            txt_encoding: "UTF-8".to_string(),
            source_encoding: "auto".to_string(),
            target_encoding: "UTF-8".to_string(),
            zhconv_target: LangType::ZhTw,
            running: false,
            result: None,
            error: None,
        }
    }
}

/// 工具箱页面 entity。
pub struct ToolsPage {
    model: Entity<AppModel>,
    kind: ToolKind,
    forms: [ToolForm; 3],
    regex_input: Entity<InputState>,
}

impl ToolsPage {
    /// 建页：编码默认沿用下载设置里的 TXT 编码。
    pub fn new(model: Entity<AppModel>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let txt_encoding = model.read(cx).config.download.txt_encoding.clone();
        let placeholder: SharedString = t!("Tools.convert.chapter_regex_placeholder").into();
        let regex_input = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
        let mut forms = [
            ToolForm::default(),
            ToolForm::default(),
            ToolForm::default(),
        ];
        forms[ToolKind::Convert.index()].txt_encoding = txt_encoding;
        Self {
            model,
            kind: ToolKind::Convert,
            forms,
            regex_input,
        }
    }

    /// 选源文件；选中后把输出目录默认设到同目录，并对文本文档探测编码。
    fn pick_source(&self, cx: &Context<Self>) {
        let kind = self.kind;
        let page = cx.entity().downgrade();
        cx.spawn(async move |_weak, async_cx| {
            let file = rfd::AsyncFileDialog::new()
                .add_filter(t!("Tools.pick.filter").as_ref(), SOURCE_EXTENSIONS)
                .set_title(t!("Tools.pick.title").as_ref())
                .pick_file()
                .await;
            let Some(file) = file else {
                return;
            };
            let path = file.path().to_path_buf();
            let detect_text = supports_chapter_regex(&path);
            let _ = page.update(async_cx, |this, cx| {
                let form = &mut this.forms[kind.index()];
                form.source = Some(path.clone());
                form.out_dir = path.parent().map(Path::to_path_buf);
                form.detected = None;
                form.result = None;
                form.error = None;
                cx.notify();

                if !detect_text {
                    return;
                }
                let detect_path = path;
                cx.spawn(async move |this, async_cx| {
                    let detected = async_cx
                        .background_executor()
                        .spawn(async move { detect_file_encoding(&detect_path) })
                        .await;
                    if let Ok(encoding) = detected {
                        let name: SharedString = encoding.name().into();
                        let _ = this.update(async_cx, |this, cx| {
                            this.forms[kind.index()].detected = Some(name);
                            cx.notify();
                        });
                    }
                })
                .detach();
            });
        })
        .detach();
    }

    /// 选输出目录，对话框默认停在当前输出目录或源文件目录。
    fn pick_out_dir(&self, cx: &Context<Self>) {
        let kind = self.kind;
        let form = &self.forms[kind.index()];
        let start = form.out_dir.clone().or_else(|| {
            form.source
                .as_ref()
                .and_then(|path| path.parent())
                .map(Path::to_path_buf)
        });
        let page = cx.entity().downgrade();
        cx.spawn(async move |_weak, async_cx| {
            let mut dialog =
                rfd::AsyncFileDialog::new().set_title(t!("Tools.pick.choose_folder").as_ref());
            if let Some(start) = start {
                dialog = dialog.set_directory(start);
            }
            let Some(folder) = dialog.pick_folder().await else {
                return;
            };
            let path = folder.path().to_path_buf();
            let _ = page.update(async_cx, |this, cx| {
                this.forms[kind.index()].out_dir = Some(path);
                cx.notify();
            });
        })
        .detach();
    }

    /// 后台执行当前工具；结束回写结果或错误。
    fn run_tool(&mut self, cx: &mut Context<Self>) {
        let kind = self.kind;
        let form = self.forms[kind.index()].clone();
        if form.running {
            return;
        }
        let Some(source) = form.source.clone() else {
            return;
        };
        let out_dir = form
            .out_dir
            .clone()
            .or_else(|| source.parent().map(Path::to_path_buf))
            .unwrap_or_else(|| PathBuf::from("."));
        let chapter_regex = if kind == ToolKind::Convert && supports_chapter_regex(&source) {
            let value = self.regex_input.read(cx).value().to_string();
            let trimmed = value.trim().to_string();
            (!trimmed.is_empty()).then_some(trimmed)
        } else {
            None
        };
        let request = match kind {
            ToolKind::Convert => ToolRequest::Convert {
                source,
                out_dir,
                target: form.target_format,
                txt_encoding: form.txt_encoding.clone(),
                source_encoding: normalize_encoding(&form.source_encoding),
                chapter_regex,
            },
            ToolKind::Encoding => ToolRequest::Encoding {
                source,
                out_dir,
                source_encoding: normalize_encoding(&form.source_encoding),
                target_encoding: form.target_encoding.clone(),
            },
            ToolKind::Zhconv => ToolRequest::Zhconv {
                source,
                out_dir,
                target: form.zhconv_target,
            },
        };

        let model = self.model.clone();
        {
            let form = &mut self.forms[kind.index()];
            form.running = true;
            form.result = None;
            form.error = None;
        }
        cx.notify();

        cx.spawn(async move |this, async_cx| {
            let outcome = async_cx
                .background_executor()
                .spawn(async move { run(request) })
                .await;
            let _ = this.update(async_cx, |this, cx| {
                let form = &mut this.forms[kind.index()];
                form.running = false;
                match outcome {
                    Ok(path) => {
                        form.result = Some(path.clone());
                        let message = t!("Tools.toast.success", name = path.display().to_string())
                            .to_string();
                        model.update(cx, |model, _cx| model.push_success(message));
                    }
                    Err(error) => {
                        let summary: SharedString = t!(error.message_key()).into();
                        let detail = error.detail();
                        form.error = Some((summary.clone(), detail.clone()));
                        model.update(cx, |model, _cx| {
                            model.push_error(format!("{summary}: {detail}"));
                        });
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn render_rail(&self, cx: &Context<Self>) -> impl IntoElement {
        v_flex()
            .w(px(220.0))
            .flex_none()
            .gap_1()
            .p_2()
            .rounded(cx.theme().radius)
            .bg(cx.theme().muted)
            .children(ToolKind::ALL.iter().map(|kind| {
                let kind = *kind;
                let selected = self.kind == kind;
                let mut item = v_flex().gap_1().child(
                    Button::new(("tool-rail", kind.index() as u64))
                        .ghost()
                        .w_full()
                        .selected(selected)
                        .icon(Icon::new(kind.icon()))
                        .label(t!(kind.label_key()))
                        .on_click(cx.listener(move |this, _, _window, cx| {
                            this.kind = kind;
                            cx.notify();
                        })),
                );
                if selected {
                    item = item.child(
                        div()
                            .px_2()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(t!(kind.description_key()).to_string()),
                    );
                }
                item
            }))
    }

    fn render_form(&self, cx: &Context<Self>) -> impl IntoElement {
        let form = &self.forms[self.kind.index()];
        v_flex()
            .flex_1()
            .gap_4()
            .child(
                PageHeader::new(t!("Tools.page_title").into())
                    .subtitle(t!(self.kind.description_key()).into()),
            )
            .child(self.render_source_row(form, cx))
            .child(self.render_out_dir_row(form, cx))
            .child(self.render_options(cx))
            .child(self.render_actions(form, cx))
    }

    fn render_source_row(&self, form: &ToolForm, cx: &Context<Self>) -> impl IntoElement {
        let path_text: SharedString = form.source.as_ref().map_or_else(
            || t!("Tools.pick.none").into(),
            |path| path.display().to_string().into(),
        );
        let mut column = v_flex().gap_1().child(
            h_flex()
                .gap_2()
                .items_center()
                .child(
                    div()
                        .flex_1()
                        .text_sm()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .child(path_text),
                )
                .child(
                    Button::new(("tool-pick-source", self.kind.index() as u64))
                        .small()
                        .label(t!("Tools.pick.choose_file"))
                        .on_click(cx.listener(|this, _, _window, cx| this.pick_source(cx))),
                ),
        );
        if let Some(detected) = &form.detected {
            column = column.child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(t!("Tools.pick.detected", encoding = detected.to_string()).to_string()),
            );
        }
        column
    }

    fn render_out_dir_row(&self, form: &ToolForm, cx: &Context<Self>) -> impl IntoElement {
        let dir_text: SharedString = form.out_dir.as_ref().map_or_else(
            || t!("Tools.pick.out_dir_default").into(),
            |path| path.display().to_string().into(),
        );
        h_flex()
            .gap_2()
            .items_center()
            .child(
                div()
                    .flex_1()
                    .text_sm()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .child(dir_text),
            )
            .child(
                Button::new(("tool-pick-out", self.kind.index() as u64))
                    .small()
                    .label(t!("Tools.pick.choose_folder"))
                    .on_click(cx.listener(|this, _, _window, cx| this.pick_out_dir(cx))),
            )
    }

    fn render_options(&self, cx: &Context<Self>) -> impl IntoElement {
        let form = &self.forms[self.kind.index()];
        let mut panel = v_flex().gap_3();
        match self.kind {
            ToolKind::Convert => {
                let mut row = h_flex().gap_2();
                for (index, format) in [
                    ExportFormat::Epub,
                    ExportFormat::Txt,
                    ExportFormat::Html,
                    ExportFormat::Pdf,
                    ExportFormat::Markdown,
                ]
                .into_iter()
                .enumerate()
                {
                    let selected = form.target_format == format;
                    row = row.child(
                        Button::new(("tool-format", index as u64))
                            .small()
                            .ghost()
                            .selected(selected)
                            .label(t!(format_key(format)))
                            .on_click(cx.listener(move |this, _, _window, cx| {
                                this.forms[ToolKind::Convert.index()].target_format = format;
                                cx.notify();
                            })),
                    );
                }
                panel = panel.child(
                    v_flex()
                        .gap_2()
                        .child(
                            div()
                                .text_sm()
                                .child(t!("Tools.convert.output_format").to_string()),
                        )
                        .child(row),
                );
                if form.target_format == ExportFormat::Txt {
                    panel = panel.child(
                        v_flex()
                            .gap_2()
                            .child(
                                div()
                                    .text_sm()
                                    .child(t!("Tools.convert.txt_encoding").to_string()),
                            )
                            .child(chip_row(
                                "tool-txt-enc",
                                TARGET_ENCODINGS,
                                &form.txt_encoding,
                                |page, value| {
                                    page.forms[ToolKind::Convert.index()].txt_encoding = value;
                                },
                                cx,
                            )),
                    );
                }
                if form.source.as_deref().is_some_and(supports_chapter_regex) {
                    panel = panel
                        .child(
                            v_flex()
                                .gap_2()
                                .child(
                                    div()
                                        .text_sm()
                                        .child(t!("Tools.encoding.source_encoding").to_string()),
                                )
                                .child(chip_row(
                                    "tool-conv-src-enc",
                                    SOURCE_ENCODINGS,
                                    &form.source_encoding,
                                    |page, value| {
                                        page.forms[ToolKind::Convert.index()].source_encoding =
                                            value;
                                    },
                                    cx,
                                )),
                        )
                        .child(
                            v_flex()
                                .gap_2()
                                .child(
                                    div()
                                        .text_sm()
                                        .child(t!("Tools.convert.chapter_regex").to_string()),
                                )
                                .child(Input::new(&self.regex_input).w_full()),
                        );
                }
            }
            ToolKind::Encoding => {
                panel = panel
                    .child(
                        v_flex()
                            .gap_2()
                            .child(
                                div()
                                    .text_sm()
                                    .child(t!("Tools.encoding.source_encoding").to_string()),
                            )
                            .child(chip_row(
                                "tool-src-enc",
                                SOURCE_ENCODINGS,
                                &form.source_encoding,
                                |page, value| {
                                    page.forms[ToolKind::Encoding.index()].source_encoding = value;
                                },
                                cx,
                            )),
                    )
                    .child(
                        v_flex()
                            .gap_2()
                            .child(
                                div()
                                    .text_sm()
                                    .child(t!("Tools.encoding.target_encoding").to_string()),
                            )
                            .child(chip_row(
                                "tool-dst-enc",
                                TARGET_ENCODINGS,
                                &form.target_encoding,
                                |page, value| {
                                    page.forms[ToolKind::Encoding.index()].target_encoding = value;
                                },
                                cx,
                            )),
                    );
            }
            ToolKind::Zhconv => {
                let mut row = h_flex().gap_2();
                for (index, (target, key)) in [
                    (LangType::ZhCn, "Tools.zhconv.zh_cn"),
                    (LangType::ZhTw, "Tools.zhconv.zh_tw"),
                    (LangType::ZhHant, "Tools.zhconv.zh_hant"),
                ]
                .into_iter()
                .enumerate()
                {
                    row = row.child(
                        Button::new(("tool-zhconv", index as u64))
                            .small()
                            .ghost()
                            .selected(form.zhconv_target == target)
                            .label(t!(key))
                            .on_click(cx.listener(move |this, _, _window, cx| {
                                this.forms[ToolKind::Zhconv.index()].zhconv_target = target;
                                cx.notify();
                            })),
                    );
                }
                panel = panel.child(
                    v_flex()
                        .gap_2()
                        .child(div().text_sm().child(t!("Tools.zhconv.target").to_string()))
                        .child(row),
                );
            }
        }
        panel
    }

    fn render_actions(&self, form: &ToolForm, cx: &Context<Self>) -> impl IntoElement {
        let label: SharedString = if form.running {
            t!("Tools.run.running").into()
        } else {
            t!("Tools.run.start").into()
        };
        let mut column = v_flex().gap_2().child(
            Button::new(("tool-run", self.kind.index() as u64))
                .label(label)
                .loading(form.running)
                .disabled(form.source.is_none() || form.running)
                .on_click(cx.listener(|this, _, _window, cx| this.run_tool(cx))),
        );
        if let Some(result) = &form.result {
            let result = result.clone();
            column = column.child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .child(result.display().to_string()),
                    )
                    .child(
                        Button::new("tool-reveal")
                            .small()
                            .ghost()
                            .label(t!("Tools.run.reveal"))
                            .on_click(cx.listener(move |_this, _, _window, _cx| {
                                if let Err(error) = crate::utils::system::reveal_in_folder(&result)
                                {
                                    tracing::warn!("reveal_in_folder failed: {error:#}");
                                }
                            })),
                    ),
            );
        }
        if let Some((summary, detail)) = &form.error {
            column = column.child(
                v_flex()
                    .gap_1()
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().danger)
                            .child(summary.clone()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(detail.clone()),
                    ),
            );
        }
        column
    }
}

impl Render for ToolsPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .size_full()
            .p_6()
            .gap_4()
            .child(self.render_rail(cx))
            .child(self.render_form(cx))
    }
}

/// 一排单选胶囊按钮：点哪项就把 `String` 值交给 `on_pick`。
fn chip_row(
    prefix: &'static str,
    options: &'static [&'static str],
    selected: &str,
    on_pick: impl Fn(&mut ToolsPage, String) + 'static,
    cx: &Context<ToolsPage>,
) -> impl IntoElement {
    let on_pick: std::rc::Rc<dyn Fn(&mut ToolsPage, String)> = std::rc::Rc::new(on_pick);
    let mut row = h_flex().gap_2();
    for (index, option) in options.iter().enumerate() {
        let is_selected = *option == selected;
        let value = (*option).to_string();
        let on_pick = on_pick.clone();
        row = row.child(
            Button::new((prefix, index as u64))
                .small()
                .ghost()
                .selected(is_selected)
                .label(*option)
                .on_click(cx.listener(move |this, _, _window, cx| {
                    on_pick(this, value.clone());
                    cx.notify();
                })),
        );
    }
    row
}

/// 是否为可切章的纯文本文档（TXT / Markdown / HTML）。
fn supports_chapter_regex(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("txt" | "md" | "markdown" | "html" | "htm")
    )
}

/// 空串 / `auto` 交给后端探测。
fn normalize_encoding(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty() && trimmed != "auto").then(|| trimmed.to_string())
}

const fn format_key(format: ExportFormat) -> &'static str {
    match format {
        ExportFormat::Epub => "Tools.format.epub",
        ExportFormat::Txt => "Tools.format.txt",
        ExportFormat::Html => "Tools.format.html",
        ExportFormat::Pdf => "Tools.format.pdf",
        ExportFormat::Markdown => "Tools.format.markdown",
    }
}
