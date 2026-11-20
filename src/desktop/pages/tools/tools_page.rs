//! 工具箱：格式转换 / 编码转换 / 简繁转换。
//!
//! 顶部一排工具卡片 + 下方表单；三种工具共享「源文件 / 输出目录 / 运行 / 结果」骨架，
//! 只有选项区不同。任务在 `background_executor` 上跑，避免阻塞 UI。

use std::path::{Path, PathBuf};

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::progress::Progress;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{ActiveTheme as _, Disableable, Icon, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AppContext, Context, Entity, FontWeight, InteractiveElement as _, IntoElement, ParentElement,
    Render, SharedString, StatefulInteractiveElement as _, Styled, Window, div, px,
};
use rust_i18n::t;
use tokio::sync::mpsc;

use crate::config::{ExportFormat, LangType};
use crate::desktop::components::PageHeader;
use crate::desktop::model::AppModel;
use crate::tools::{
    StageWeights, ToolProgress, ToolRequest, ToolStage, detect_file_encoding, run_with_progress,
};

/// 支持的源文件扩展名（文件选择器过滤用）。
const SOURCE_EXTENSIONS: &[&str] = &["txt", "md", "markdown", "html", "htm", "epub", "zip"];

/// 可选的源编码；`auto` 表示自动探测。
const SOURCE_ENCODINGS: &[&str] = &["auto", "UTF-8", "GBK", "GB18030", "Big5", "Big5-HKSCS"];

/// 可选的输出编码；不含 UTF-16（`encoding_rs` 的 UTF-16 只支持解码）。
const TARGET_ENCODINGS: &[&str] = &["UTF-8", "GBK", "GB18030", "Big5", "Big5-HKSCS"];

/// 工具产出是否落在下载目录里 —— 落在里面书库扫描才能发现，需要清掉扫描哨兵。
///
/// 扫描不递归，只认下载目录的**直接子项**，所以比较的是产出的父目录与下载根。
/// 两侧都按 `normalize_path_key` 归一：Windows 上 `Path` 相等大小写敏感，
/// 直接用 `Path` 比较会漏判。
#[must_use]
fn output_lands_in_download_dir(output: &Path, download_path: &str) -> bool {
    if download_path.trim().is_empty() {
        return false;
    }
    let Some(parent) = output.parent() else {
        return false;
    };
    crate::library::normalize_path_key(parent)
        == crate::library::normalize_path_key(Path::new(download_path))
}

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
    progress: Option<ToolProgress>,
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
            progress: None,
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
            form.progress = None;
            form.result = None;
            form.error = None;
        }
        cx.notify();

        // 进度走 channel：工作线程只发不碰 UI，前台再逐条回写，避免跨线程改 entity。
        let (progress_tx, mut progress_rx) = mpsc::unbounded_channel::<ToolProgress>();
        cx.spawn(async move |this, async_cx| {
            while let Some(progress) = progress_rx.recv().await {
                let _ = this.update(async_cx, |this, cx| {
                    this.forms[kind.index()].progress = Some(progress);
                    cx.notify();
                });
            }
        })
        .detach();

        cx.spawn(async move |this, async_cx| {
            let outcome = async_cx
                .background_executor()
                .spawn(async move {
                    run_with_progress(request, |progress| {
                        let _ = progress_tx.send(progress);
                    })
                })
                .await;
            let _ = this.update(async_cx, |this, cx| {
                let form = &mut this.forms[kind.index()];
                form.running = false;
                form.progress = None;
                match outcome {
                    Ok(path) => {
                        form.result = Some(path.clone());
                        let message = t!("Tools.toast.success", name = path.display().to_string())
                            .to_string();
                        model.update(cx, |model, _cx| {
                            model.push_success(message);
                            if output_lands_in_download_dir(
                                &path,
                                &model.config.download.download_path,
                            ) {
                                // 产出落在扫描范围内：下次渲染书库页时重扫。
                                model.library.scanned_dir = None;
                            }
                        });
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

    /// 一排工具卡片（同屏 tab）：点卡片切换下方表单，选中卡片描边 + 高亮底色 + 右侧圆点。
    fn render_kind_cards(&self, cx: &Context<Self>) -> impl IntoElement {
        h_flex()
            .gap_3()
            .items_stretch()
            .children(ToolKind::ALL.iter().map(|kind| {
                let kind = *kind;
                let selected = self.kind == kind;
                let theme = cx.theme();
                // accent 在多数主题里是中性灰，选中态用 primary 才有明显对比。
                let highlight = theme.primary;
                let hover_bg = theme.muted;
                let border = if selected { highlight } else { theme.border };
                let icon_color = if selected {
                    highlight
                } else {
                    theme.muted_foreground
                };
                v_flex()
                    .id(("tool-card", kind.index() as u64))
                    .flex_1()
                    .min_w_0()
                    .gap_2()
                    .p_3()
                    .rounded(theme.radius)
                    .border_1()
                    .border_color(border)
                    .when(selected, |card| card.bg(hover_bg))
                    .cursor_pointer()
                    .hover(|style| style.bg(hover_bg))
                    .child(
                        h_flex()
                            .w_full()
                            .items_center()
                            .justify_between()
                            .gap_2()
                            .child(
                                h_flex()
                                    .min_w_0()
                                    .items_center()
                                    .gap_2()
                                    .child(Icon::new(kind.icon()).text_color(icon_color))
                                    .child(
                                        div()
                                            .min_w_0()
                                            .overflow_hidden()
                                            .whitespace_nowrap()
                                            .text_ellipsis()
                                            .text_sm()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child(t!(kind.label_key()).to_string()),
                                    ),
                            )
                            .when(selected, |row| {
                                row.child(div().size(px(6.0)).rounded_full().bg(highlight))
                            }),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(t!(kind.description_key()).to_string()),
                    )
                    .on_click(cx.listener(move |this, _, _window, cx| {
                        this.kind = kind;
                        cx.notify();
                    }))
            }))
    }

    fn render_form(&self, cx: &Context<Self>) -> impl IntoElement {
        let form = &self.forms[self.kind.index()];
        let theme = cx.theme();
        v_flex()
            .w_full()
            .min_w_0()
            .gap_4()
            .p_5()
            .rounded(theme.radius)
            .border_1()
            .border_color(theme.border)
            .child(self.render_source_row(form, cx))
            .child(Self::divider(cx))
            .child(self.render_out_dir_row(form, cx))
            .child(Self::divider(cx))
            .child(self.render_options(cx))
            .child(Self::divider(cx))
            .child(self.render_actions(form, cx))
    }

    /// 字段块：小号灰标签 + 内容，与上方卡片的说明文字同一视觉层级。
    fn field(
        label: SharedString,
        content: impl IntoElement,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        v_flex()
            .w_full()
            .min_w_0()
            .gap_2()
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(label),
            )
            .child(content)
    }

    fn divider(cx: &Context<Self>) -> impl IntoElement {
        div().h(px(1.0)).w_full().bg(cx.theme().border.opacity(0.6))
    }

    /// 只读路径框：浅底色 + 圆角，高度和默认按钮（32px）对齐。
    fn path_box(text: SharedString, cx: &Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        h_flex()
            .flex_1()
            .min_w_0()
            .h(px(32.0))
            .items_center()
            .px_3()
            .rounded(theme.radius)
            .bg(theme.muted)
            .text_sm()
            .overflow_hidden()
            .whitespace_nowrap()
            .text_ellipsis()
            .child(text)
    }

    fn render_source_row(&self, form: &ToolForm, cx: &Context<Self>) -> impl IntoElement {
        let path_text: SharedString = form.source.as_ref().map_or_else(
            || t!("Tools.pick.none").into(),
            |path| path.display().to_string().into(),
        );
        let mut column = v_flex().w_full().min_w_0().gap_2().child(
            h_flex()
                .gap_2()
                .items_center()
                .child(Self::path_box(path_text, cx))
                .child(
                    Button::new(("tool-pick-source", self.kind.index() as u64))
                        .flex_none()
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
        Self::field(t!("Tools.field.source").into(), column, cx)
    }

    fn render_out_dir_row(&self, form: &ToolForm, cx: &Context<Self>) -> impl IntoElement {
        let dir_text: SharedString = form.out_dir.as_ref().map_or_else(
            || t!("Tools.pick.out_dir_default").into(),
            |path| path.display().to_string().into(),
        );
        Self::field(
            t!("Tools.field.out_dir").into(),
            h_flex()
                .gap_2()
                .items_center()
                .child(Self::path_box(dir_text, cx))
                .child(
                    Button::new(("tool-pick-out", self.kind.index() as u64))
                        .flex_none()
                        .label(t!("Tools.pick.choose_folder"))
                        .on_click(cx.listener(|this, _, _window, cx| this.pick_out_dir(cx))),
                ),
            cx,
        )
    }

    fn render_options(&self, cx: &Context<Self>) -> impl IntoElement {
        let form = &self.forms[self.kind.index()];
        let mut panel = v_flex().w_full().min_w_0().gap_4();
        match self.kind {
            ToolKind::Convert => {
                let mut row = h_flex().flex_wrap().gap_2();
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
                    row = row.child(option_chip(
                        ("tool-format", index as u64),
                        t!(format_key(format)).to_string(),
                        form.target_format == format,
                        move |page| {
                            page.forms[ToolKind::Convert.index()].target_format = format;
                        },
                        cx,
                    ));
                }
                panel = panel.child(Self::field(
                    t!("Tools.convert.output_format").into(),
                    row,
                    cx,
                ));
                if form.target_format == ExportFormat::Txt {
                    panel = panel.child(Self::field(
                        t!("Tools.convert.txt_encoding").into(),
                        chip_row(
                            "tool-txt-enc",
                            TARGET_ENCODINGS,
                            &form.txt_encoding,
                            |page, value| {
                                page.forms[ToolKind::Convert.index()].txt_encoding = value;
                            },
                            cx,
                        ),
                        cx,
                    ));
                }
                if form.source.as_deref().is_some_and(supports_chapter_regex) {
                    panel = panel
                        .child(Self::field(
                            t!("Tools.encoding.source_encoding").into(),
                            chip_row(
                                "tool-conv-src-enc",
                                SOURCE_ENCODINGS,
                                &form.source_encoding,
                                |page, value| {
                                    page.forms[ToolKind::Convert.index()].source_encoding = value;
                                },
                                cx,
                            ),
                            cx,
                        ))
                        .child(Self::field(
                            t!("Tools.convert.chapter_regex").into(),
                            Input::new(&self.regex_input).w_full(),
                            cx,
                        ));
                }
            }
            ToolKind::Encoding => {
                panel = panel
                    .child(Self::field(
                        t!("Tools.encoding.source_encoding").into(),
                        chip_row(
                            "tool-src-enc",
                            SOURCE_ENCODINGS,
                            &form.source_encoding,
                            |page, value| {
                                page.forms[ToolKind::Encoding.index()].source_encoding = value;
                            },
                            cx,
                        ),
                        cx,
                    ))
                    .child(Self::field(
                        t!("Tools.encoding.target_encoding").into(),
                        chip_row(
                            "tool-dst-enc",
                            TARGET_ENCODINGS,
                            &form.target_encoding,
                            |page, value| {
                                page.forms[ToolKind::Encoding.index()].target_encoding = value;
                            },
                            cx,
                        ),
                        cx,
                    ));
            }
            ToolKind::Zhconv => {
                let mut row = h_flex().flex_wrap().gap_2();
                for (index, (target, key)) in [
                    (LangType::ZhCn, "Tools.zhconv.zh_cn"),
                    (LangType::ZhTw, "Tools.zhconv.zh_tw"),
                    (LangType::ZhHant, "Tools.zhconv.zh_hant"),
                ]
                .into_iter()
                .enumerate()
                {
                    row = row.child(option_chip(
                        ("tool-zhconv", index as u64),
                        t!(key).to_string(),
                        form.zhconv_target == target,
                        move |page| {
                            page.forms[ToolKind::Zhconv.index()].zhconv_target = target;
                        },
                        cx,
                    ));
                }
                panel = panel.child(Self::field(t!("Tools.zhconv.target").into(), row, cx));
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
        let mut column = v_flex().w_full().min_w_0().gap_3();
        if let Some(result) = &form.result {
            let result = result.clone();
            column = column.child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .w_full()
                    .child(Self::path_box(result.display().to_string().into(), cx))
                    .child(
                        Button::new("tool-reveal")
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
        if form.running {
            let progress = form
                .progress
                .unwrap_or(ToolProgress::new(ToolStage::Loading, 0, 1));
            // 权重随工具 / 目标格式变化：PDF 写盘占九成耗时，TXT 几乎只有读写。
            let weights = match self.kind {
                ToolKind::Convert => StageWeights::for_format(form.target_format),
                ToolKind::Encoding | ToolKind::Zhconv => StageWeights::plain_file(),
            };
            let stage: SharedString = t!(progress.stage.label_key()).into();
            let detail: SharedString = if progress.total > 1 {
                format!("{stage} {}/{}", progress.done, progress.total).into()
            } else {
                stage
            };
            let percent: SharedString = format!("{:.0}%", progress.percent(weights)).into();
            column = column.child(
                v_flex()
                    .w_full()
                    .gap_2()
                    .child(
                        h_flex()
                            .w_full()
                            .justify_between()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(detail)
                            .child(percent),
                    )
                    .child(
                        Progress::new(("tool-progress", self.kind.index() as u64))
                            .value(progress.percent(weights)),
                    ),
            );
        }
        column.child(
            h_flex().w_full().justify_end().child(
                Button::new(("tool-run", self.kind.index() as u64))
                    .primary()
                    .label(label)
                    .loading(form.running)
                    .disabled(form.source.is_none() || form.running)
                    .on_click(cx.listener(|this, _, _window, cx| this.run_tool(cx))),
            ),
        )
    }
}

impl Render for ToolsPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .p_6()
            .gap_4()
            .child(
                PageHeader::new(t!("Tools.page_title").into())
                    .subtitle(t!("Tools.page_subtitle").into()),
            )
            .child(
                div()
                    .id("tools-scroll")
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .min_w_0()
                    .overflow_y_scrollbar()
                    .child(
                        v_flex()
                            .w_full()
                            .min_w_0()
                            .gap_5()
                            .child(self.render_kind_cards(cx))
                            .child(self.render_form(cx)),
                    ),
            )
    }
}

/// 一排单选胶囊按钮：点哪项就把 `String` 值交给 `on_pick`。
/// 单选胶囊：与上方工具卡片共用一套描边语言，选中态用主题主色。
fn option_chip(
    id: (&'static str, u64),
    label: impl Into<SharedString>,
    selected: bool,
    on_pick: impl Fn(&mut ToolsPage) + 'static,
    cx: &Context<ToolsPage>,
) -> impl IntoElement {
    let theme = cx.theme();
    let accent = theme.primary;
    let muted_bg = theme.muted;
    let border = theme.border;
    div()
        .id(id)
        .px_3()
        .py_1p5()
        .rounded(theme.radius)
        .border_1()
        .border_color(if selected { accent } else { border })
        .when(selected, |chip| chip.bg(muted_bg))
        .cursor_pointer()
        .hover(move |style| style.bg(muted_bg))
        .text_sm()
        .when(selected, move |chip| chip.text_color(accent))
        .child(label.into())
        .on_click(cx.listener(move |this, _, _window, cx| {
            on_pick(this);
            cx.notify();
        }))
}

fn chip_row(
    prefix: &'static str,
    options: &'static [&'static str],
    selected: &str,
    on_pick: impl Fn(&mut ToolsPage, String) + 'static,
    cx: &Context<ToolsPage>,
) -> impl IntoElement {
    let on_pick: std::rc::Rc<dyn Fn(&mut ToolsPage, String)> = std::rc::Rc::new(on_pick);
    let mut row = h_flex().flex_wrap().gap_2();
    for (index, option) in options.iter().enumerate() {
        let value = (*option).to_string();
        let on_pick = on_pick.clone();
        row = row.child(option_chip(
            (prefix, index as u64),
            *option,
            *option == selected,
            move |page| on_pick(page, value.clone()),
            cx,
        ));
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

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::*;

    #[test]
    fn output_lands_in_download_dir_only_for_direct_children() {
        let download = tempfile::tempdir().unwrap();
        let inside = download.path().join("converted.txt");
        let nested = download.path().join("sub").join("converted.txt");
        let elsewhere = tempfile::tempdir().unwrap().path().join("converted.txt");

        assert!(output_lands_in_download_dir(
            &inside,
            &download.path().to_string_lossy()
        ));
        assert!(
            !output_lands_in_download_dir(&nested, &download.path().to_string_lossy()),
            "工具栏产出是目录直接子项，子目录里的文件扫描根也不会直接列出"
        );
        assert!(!output_lands_in_download_dir(
            &elsewhere,
            &download.path().to_string_lossy()
        ));
        assert!(
            !output_lands_in_download_dir(&inside, "   "),
            "下载目录为空时不能扫，也不能误判命中"
        );
    }
}
