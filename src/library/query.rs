//! 书架筛选与排序：纯函数，不持有状态。

use super::book::{Book, ShelfStatus};

/// 书架状态筛选（与 `ShelfStatus` 区分：`All` 不是一种书籍状态）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ShelfStatusFilter {
    /// 不按状态过滤。
    #[default]
    All,
    /// 未读。
    Unread,
    /// 在读。
    Reading,
    /// 已读。
    Finished,
}

impl ShelfStatusFilter {
    /// 该筛选是否接受这本书。
    #[must_use]
    pub fn accepts(self, book: &Book) -> bool {
        match self {
            Self::All => true,
            Self::Unread => book.status() == ShelfStatus::Unread,
            Self::Reading => book.status() == ShelfStatus::Reading,
            Self::Finished => book.status() == ShelfStatus::Finished,
        }
    }

    /// 文案 key（`Library.filter.*`）。
    #[must_use]
    pub const fn label_key(self) -> &'static str {
        match self {
            Self::All => "Library.filter.all",
            Self::Unread => "Library.filter.unread",
            Self::Reading => "Library.filter.reading",
            Self::Finished => "Library.filter.finished",
        }
    }
}

/// 书架排序。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ShelfSort {
    /// 最近阅读，未打开的书排在后面。
    #[default]
    LastOpened,
    /// 最近添加。
    AddedAt,
    /// 书名（ASCII 大小写不敏感）。
    Title,
    /// 文件大小，从大到小。
    Size,
}

impl ShelfSort {
    /// 全部可选项，UI 下拉按此顺序渲染。
    pub const ALL: [Self; 4] = [Self::LastOpened, Self::AddedAt, Self::Title, Self::Size];

    /// 文案 key（`Library.sort.*`）。
    #[must_use]
    pub const fn label_key(self) -> &'static str {
        match self {
            Self::LastOpened => "Library.sort.last_opened",
            Self::AddedAt => "Library.sort.added_at",
            Self::Title => "Library.sort.title",
            Self::Size => "Library.sort.size",
        }
    }
}

/// 书架视图查询：搜索 + 状态筛选 + 收藏开关 + 排序。
#[derive(Debug, Clone, Default)]
pub struct ShelfQuery {
    /// 已 `trim`；空串 = 不过滤。
    pub text: String,
    pub status: ShelfStatusFilter,
    pub favorite_only: bool,
    pub sort: ShelfSort,
}

impl ShelfQuery {
    /// 命中书名 / 作者 / 文件名（ASCII 大小写不敏感）；收藏开关要求 `book.favorite`。
    /// 隐藏书（`hidden`）任何查询都不返回 —— 「移出书架」的扫描来源靠这条生效。
    #[must_use]
    pub fn matches(&self, book: &Book) -> bool {
        if book.hidden {
            return false;
        }
        if self.favorite_only && !book.favorite {
            return false;
        }
        if !self.status.accepts(book) {
            return false;
        }
        let needle = self.text.trim();
        if needle.is_empty() {
            return true;
        }
        let needle = needle.to_ascii_lowercase();
        let file_name = std::path::Path::new(&book.path)
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default();
        book.title.to_ascii_lowercase().contains(&needle)
            || book.author.to_ascii_lowercase().contains(&needle)
            || file_name.to_ascii_lowercase().contains(&needle)
    }
}

/// 过滤 + 排序，返回借用而非拷贝。
#[must_use]
pub fn visible_books<'a>(books: &'a [Book], query: &ShelfQuery) -> Vec<&'a Book> {
    let mut out: Vec<&Book> = books.iter().filter(|book| query.matches(book)).collect();
    match query.sort {
        ShelfSort::LastOpened => out.sort_by(|a, b| {
            b.last_opened_unix
                .cmp(&a.last_opened_unix)
                .then_with(|| a.id.cmp(&b.id))
        }),
        ShelfSort::AddedAt => out.sort_by(|a, b| {
            b.added_unix
                .cmp(&a.added_unix)
                .then_with(|| a.id.cmp(&b.id))
        }),
        ShelfSort::Title => out.sort_by(|a, b| {
            a.title
                .to_ascii_lowercase()
                .cmp(&b.title.to_ascii_lowercase())
                .then_with(|| a.id.cmp(&b.id))
        }),
        ShelfSort::Size => out.sort_by(|a, b| {
            b.size_bytes
                .cmp(&a.size_bytes)
                .then_with(|| a.id.cmp(&b.id))
        }),
    }
    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::*;
    use crate::library::book::{BookFormat, BookOrigin, ReadingProgress};

    fn book(id: &str, title: &str, author: &str, opened: Option<i64>) -> Book {
        Book {
            id: id.to_string(),
            path_key: id.to_string(),
            path: format!("C:/books/{id}.txt"),
            format: BookFormat::Txt,
            origin: BookOrigin::Downloaded,
            source_root: None,
            title: title.to_string(),
            author: author.to_string(),
            intro: None,
            cover_file: None,
            size_bytes: 100,
            file_mtime: 0,
            added_unix: 0,
            last_opened_unix: opened,
            metadata_edited: false,
            hidden: false,
            missing: false,
            encoding: None,
            chapter_count: None,
            favorite: false,
            progress: ReadingProgress::default(),
            bookmarks: Vec::new(),
        }
    }

    #[test]
    fn text_matches_title_author_and_file_name() {
        let mut b = book("id1", "斗破苍穹", "天蚕土豆", None);
        let query = ShelfQuery {
            text: "土豆".to_string(),
            ..ShelfQuery::default()
        };
        assert!(query.matches(&b));
        let query = ShelfQuery {
            text: "id1".to_string(),
            ..ShelfQuery::default()
        };
        assert!(query.matches(&b));
        let query = ShelfQuery {
            text: "斗破".to_string(),
            ..ShelfQuery::default()
        };
        assert!(query.matches(&b));
        let query = ShelfQuery {
            text: "不存在".to_string(),
            ..ShelfQuery::default()
        };
        assert!(!query.matches(&b));
        b.author.clear();
        assert!(
            !ShelfQuery {
                text: "土豆".into(),
                ..ShelfQuery::default()
            }
            .matches(&b)
        );
    }

    #[test]
    fn status_and_favorite_filters_combine() {
        let mut unread = book("a", "A", "", None);
        unread.favorite = true;
        let mut reading = book("b", "B", "", Some(5));
        reading.progress.updated_unix = 5;
        let mut finished = book("c", "C", "", Some(5));
        finished.progress.updated_unix = 5;
        finished.progress.finished = true;
        let books = vec![unread, reading, finished];

        let ids = |status, favorite_only| {
            visible_books(
                &books,
                &ShelfQuery {
                    status,
                    favorite_only,
                    ..ShelfQuery::default()
                },
            )
            .iter()
            .map(|b| b.id.as_str())
            .collect::<Vec<_>>()
        };

        // 默认排序是 LastOpened（未打开的书排最后），同值按 id 升序。
        assert_eq!(ids(ShelfStatusFilter::All, false), vec!["b", "c", "a"]);
        assert_eq!(ids(ShelfStatusFilter::Unread, false), vec!["a"]);
        assert_eq!(ids(ShelfStatusFilter::Reading, false), vec!["b"]);
        assert_eq!(ids(ShelfStatusFilter::Finished, false), vec!["c"]);
        assert_eq!(ids(ShelfStatusFilter::All, true), vec!["a"]);
    }

    #[test]
    fn hidden_books_are_excluded() {
        let shown = book("a", "A", "", None);
        let mut hidden = book("b", "B", "", None);
        hidden.hidden = true;
        hidden.favorite = true;
        let books = vec![shown, hidden];

        // 隐藏书不参与任何查询，包括「收藏」筛选。
        assert_eq!(visible_books(&books, &ShelfQuery::default()).len(), 1);
        assert!(
            visible_books(
                &books,
                &ShelfQuery {
                    favorite_only: true,
                    ..ShelfQuery::default()
                }
            )
            .is_empty()
        );
    }

    #[test]
    fn sorts_by_last_opened_then_id() {
        let books = vec![
            book("b", "B", "", Some(10)),
            book("a", "A", "", None),
            book("c", "C", "", Some(20)),
            book("d", "D", "", Some(10)),
        ];
        let sorted = visible_books(
            &books,
            &ShelfQuery {
                sort: ShelfSort::LastOpened,
                ..ShelfQuery::default()
            },
        );
        let ids: Vec<_> = sorted.iter().map(|b| b.id.as_str()).collect();
        assert_eq!(ids, vec!["c", "b", "d", "a"]);
    }

    #[test]
    fn sorts_by_title_and_size() {
        let mut books = vec![
            book("1", "beta", "", None),
            book("2", "Alpha", "", None),
            book("3", "gamma", "", None),
        ];
        books[0].size_bytes = 30;
        books[1].size_bytes = 10;
        books[2].size_bytes = 20;

        let by_title: Vec<_> = visible_books(
            &books,
            &ShelfQuery {
                sort: ShelfSort::Title,
                ..ShelfQuery::default()
            },
        )
        .iter()
        .map(|b| b.title.as_str())
        .collect();
        assert_eq!(by_title, vec!["Alpha", "beta", "gamma"]);

        let by_size: Vec<_> = visible_books(
            &books,
            &ShelfQuery {
                sort: ShelfSort::Size,
                ..ShelfQuery::default()
            },
        )
        .iter()
        .map(|b| b.id.as_str())
        .collect();
        assert_eq!(by_size, vec!["1", "3", "2"]);
    }
}
