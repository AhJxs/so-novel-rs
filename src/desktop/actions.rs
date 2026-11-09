// `actions!` 在本模块调用, 生成的类型经 `desktop` re-export 为 `desktop::*`, 供 root.rs / nav.rs 引用。
gpui_kit::actions!(
    desktop,
    [
        /// 退出应用。
        Quit,
        /// 切到搜索页。
        ShowSearch,
        /// 切到下载任务页。
        ShowTasks,
        /// 切到本地书库页。
        ShowLibrary,
        /// 切到书源管理页。
        ShowSources,
        /// 切到设置页。
        ShowSettings,
        /// 翻到下一页。
        NextPage,
        /// 翻到上一页。
        PrevPage,
        /// 折叠 / 展开侧栏。
        ToggleSidebar,
    ]
);
// 为 actions! 生成的 PartialEq 单元结构体补充 Eq 实现。
impl Eq for Quit {}
impl Eq for ShowSearch {}
impl Eq for ShowTasks {}
impl Eq for ShowLibrary {}
impl Eq for ShowSources {}
impl Eq for ShowSettings {}
impl Eq for NextPage {}
impl Eq for PrevPage {}
impl Eq for ToggleSidebar {}
