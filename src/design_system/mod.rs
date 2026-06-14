//! 设计系统 — 配色、字体、公共 UI 组件。
//!
//! 从 `so-novel-rs` 的 `src/ui/theme.rs` 抽取，按职能拆分为子模块：
//! - `color` — 强调色 + 语义色函数
//! - `font` — CJK 字体安装 + 视觉风格
//! - `frame` — 面板 / 导航 / 标题栏 frame 工厂
//! - `button` — 各样式按钮工厂
//! - `input` — 搜索输入框 + 圆角下拉框
//! - `chip` — 统计 chip + 空态视图
//! - `toggle` — iOS 风格 toggle 开关
//! - `settings` — 设置行通用布局
//! - `theme_picker` — 主题偏好枚举 + 段控选择器

pub mod button;
pub mod chip;
pub mod color;
pub mod font;
pub mod frame;
pub mod input;
pub mod settings;
pub mod theme_picker;
pub mod toggle;
