// `actions!` 生成的类型落在调用点 (即 `desktop::*`), 供 root.rs / nav.rs 引用。
gpui_kit::actions!(
    desktop,
    [
        Quit,
        ShowSearch,
        ShowTasks,
        ShowLibrary,
        ShowSources,
        ShowSettings,
        NextPage,
        PrevPage,
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
