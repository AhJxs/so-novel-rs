//! 列表渲染缓存: 避免每帧重复 `filter + sort + slice + clone` 全表。
//!
//! `drain_loop` 每 100ms 唤醒一次, 数据没变时不应按 10fps 重做全表推导。以
//! `(page_kind, data_version, filter_signature, page_index)` 为 key 缓存最终 `Vec`,
//! 命中即复用 `Arc<Vec<…>>`。
//!
//! # 失效归属（唯一 owner）
//!
//! 每个 [`PageKind`] 只允许**一个**所有者负责失效, 避免"谁都清一点"或"没人清":
//!
//! - [`PageKind::Search`]: `SearchPage` 是唯一失效者。它在 model 的通知里看到
//!   新的 `search.results_version` 后调用 [`ListCache::retain_version`] 淘汰该页
//!   旧版本键, 并在同一处（非 render 路径）写入新快照。render 只读缓存, 未命中
//!   就直接退回未缓存切片, 绝不在渲染期写缓存。
//! - [`PageKind::Library`] / [`PageKind::Tasks`]: 目前未接入（Tasks 见
//!   `tasks_page.rs` 的 TODO）。
//! - 规则集合整体替换（切活跃书源文件 / 导入重载）是全局失效点: `AppModel` 的
//!   sources 业务方法调 [`ListCache::clear`], 一次覆盖所有页。
//!
//! 新增缓存页时必须先指定它的唯一失效者; 不要用 `render` 充当失效者。

use std::any::TypeId;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

/// 列表页种类。`u8` 足够，互斥。
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub enum PageKind {
    /// 下载文件库页。
    Library,
    /// 搜索结果页。
    Search,
    /// 下载任务页。
    Tasks,
}

/// 列表缓存 key。`elem_type` 是为了**类型擦除**: `HashMap` 只存一种 `Arc<dyn Any>`,
/// 不同 page 的元素类型不同, 靠 `TypeId` 保证不会把书架 `ShelfBook` Vec 当成
/// `SearchResult` Vec 取出来。
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub struct ListCacheKey {
    /// 属于哪个列表页。
    pub page: PageKind,
    /// 底层数据版本，数据一变就换 key。
    pub data_version: u64,
    /// 过滤条件的哈希签名。
    pub filter_sig: u64,
    /// 当前分页下标。
    pub page_index: u32,
    /// 元素类型，用于类型擦除后安全取回。
    pub elem_type: TypeId,
}

/// 单条缓存值：装下任何元素类型的 `Vec`。
type CacheValue = Arc<dyn std::any::Any + Send + Sync>;

/// 列表渲染缓存。故意不设上限 —— 每页只有几个 key, 总数最多几十条, 元素也都不重。
#[derive(Default)]
pub struct ListCache {
    map: HashMap<ListCacheKey, CacheValue>,
}

impl ListCache {
    /// 建一个空缓存。
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 取缓存。返回 `None` 表示未命中, 调用方应走原始 filter+sort+slice 路径。
    #[must_use]
    pub fn get<T: 'static + Send + Sync + Clone>(&self, key: ListCacheKey) -> Option<Arc<Vec<T>>> {
        let v = self.map.get(&key)?;
        let arc = Arc::clone(v);
        arc.downcast::<Vec<T>>().ok()
    }

    /// 写入缓存, 返回 `Arc<Vec<T>>` 句柄, 免得调用方再 clone 整张表。
    pub fn insert<T: 'static + Send + Sync>(
        &mut self,
        key: ListCacheKey,
        value: Vec<T>,
    ) -> Arc<Vec<T>> {
        let arc = Arc::new(value);
        // `arc.clone()` 在这里 unsized-coerce 成 `Arc<dyn Any + Send + Sync>`, 共享同一份分配。
        self.map.insert(key, arc.clone());
        arc
    }

    /// 清空全部缓存。规则集合整体变化（切活跃书源文件 / 导入重载）时调。
    pub fn clear(&mut self) {
        self.map.clear();
    }

    /// 淘汰某页里 `data_version` 不等于当前值的键, 只保留当前版本的快照。
    ///
    /// 单页键数 = 过滤签名 × 页数, 通常个位数; 因此这是 O(页数) 的清理, 却能让
    /// 流式搜索的旧版本"整表快照"立刻释放, 而不是无限堆积到下一次 `clear()`。
    /// 调用方（唯一失效者）见模块文档。
    pub fn retain_version(&mut self, page: PageKind, data_version: u64) {
        self.map
            .retain(|key, _| key.page != page || key.data_version == data_version);
    }

    /// 当前缓存条目数（测试用）。
    #[cfg(test)]
    #[must_use]
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// 当前是否为空（`clippy::len_without_is_empty` 要求 `len` 配套 `is_empty`）。
    #[cfg(test)]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

/// 计算过滤输入（文本 / 扩展名 / 状态）的 hash。std `Hash` + `Hasher` 即可。
#[must_use]
pub fn filter_signature(parts: &[&str]) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for p in parts {
        p.hash(&mut hasher);
    }
    hasher.finish()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
    use super::*;

    #[test]
    fn round_trip_with_correct_type() {
        let mut cache = ListCache::new();
        let key = ListCacheKey {
            page: PageKind::Library,
            data_version: 1,
            filter_sig: 0,
            page_index: 0,
            elem_type: TypeId::of::<i32>(),
        };
        cache.insert(key, vec![1_i32, 2, 3]);
        let got = cache.get::<i32>(key).expect("hit");
        assert_eq!(got.len(), 3);
        assert_eq!(got[1], 2);
    }

    #[test]
    fn wrong_typeid_misses() {
        let mut cache = ListCache::new();
        let key_i32 = ListCacheKey {
            page: PageKind::Library,
            data_version: 1,
            filter_sig: 0,
            page_index: 0,
            elem_type: TypeId::of::<i32>(),
        };
        cache.insert(key_i32, vec![1_i32, 2, 3]);
        let key_str = ListCacheKey {
            page: PageKind::Library,
            data_version: 1,
            filter_sig: 0,
            page_index: 0,
            elem_type: TypeId::of::<&str>(),
        };
        assert!(cache.get::<&str>(key_str).is_none());
    }

    #[test]
    fn clear_drops_all_entries() {
        let mut cache = ListCache::new();
        let key = ListCacheKey {
            page: PageKind::Search,
            data_version: 0,
            filter_sig: 0,
            page_index: 0,
            elem_type: TypeId::of::<u8>(),
        };
        cache.insert(key, vec![1_u8, 2, 3]);
        assert_eq!(cache.len(), 1);
        cache.clear();
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn retain_version_drops_old_versions_for_that_page_only() {
        let mut cache = ListCache::new();
        let key = |page: PageKind, data_version: u64| ListCacheKey {
            page,
            data_version,
            filter_sig: 0,
            page_index: 0,
            elem_type: TypeId::of::<u8>(),
        };
        cache.insert(key(PageKind::Search, 1), vec![1_u8]);
        cache.insert(key(PageKind::Search, 2), vec![2_u8]);
        cache.insert(key(PageKind::Library, 1), vec![3_u8]);

        cache.retain_version(PageKind::Search, 2);

        assert!(cache.get::<u8>(key(PageKind::Search, 1)).is_none());
        assert!(cache.get::<u8>(key(PageKind::Search, 2)).is_some());
        assert!(
            cache.get::<u8>(key(PageKind::Library, 1)).is_some(),
            "其他页不受影响"
        );
    }

    #[test]
    fn filter_signature_differs_for_different_inputs() {
        let a = filter_signature(&["txt", "epub"]);
        let b = filter_signature(&["epub", "txt"]);
        let c = filter_signature(&["epub"]);
        assert_ne!(a, b, "顺序应影响 hash");
        assert_ne!(a, c);
    }
}
