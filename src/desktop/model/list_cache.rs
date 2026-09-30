//! 列表渲染缓存: 避免每帧重复 `filter + sort + slice + clone` 全表。
//!
//! `pages::{library,search,tasks}` 每帧都要过滤 / 排序 / 切片 / clone 整个列表, 而
//! `drain_loop` 每 100ms 唤醒一次 —— 数据没变也按 10fps 重做。以
//! `(page_kind, data_version, filter_signature, page_index)` 为 key 缓存最终 `Vec`,
//! 命中即复用 `Arc<Vec<…>>`; 数据/状态变化时 `clear()` 整表。

use std::any::TypeId;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

/// 列表页种类。`u8` 足够，互斥。
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub enum PageKind {
    Library,
    Search,
    Tasks,
}

/// 列表缓存 key。`elem_type` 是为了**类型擦除**: `HashMap` 只存一种 `Arc<dyn Any>`,
/// 不同 page 的元素类型不同, 靠 `TypeId` 保证不会把 `LibraryEntry` Vec 当成
/// `SearchResult` Vec 取出来。
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub struct ListCacheKey {
    pub page: PageKind,
    pub data_version: u64,
    pub filter_sig: u64,
    pub page_index: u32,
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
    pub fn new() -> Self {
        Self::default()
    }

    /// 取缓存。返回 `None` 表示未命中, 调用方应走原始 filter+sort+slice 路径。
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

    /// 清空全部缓存。源数据大变化（如 `clear_results_and_caches`）时调。
    pub fn clear(&mut self) {
        self.map.clear();
    }

    /// 当前缓存条目数（测试用）。
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// 当前是否为空（`clippy::len_without_is_empty` 要求 `len` 配套 `is_empty`）。
    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

/// 计算过滤输入（文本 / 扩展名 / 状态）的 hash。std `Hash` + `Hasher` 即可。
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
    fn filter_signature_differs_for_different_inputs() {
        let a = filter_signature(&["txt", "epub"]);
        let b = filter_signature(&["epub", "txt"]);
        let c = filter_signature(&["epub"]);
        assert_ne!(a, b, "顺序应影响 hash");
        assert_ne!(a, c);
    }
}
