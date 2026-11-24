//! 与格式无关的书稿模型：加载器产出它，转换流程再把它投影成导出层 `BookDetail`。

use std::path::Path;

use crate::models::BookDetail;

/// 从本地文件加载出的书稿。
#[derive(Debug, Clone, Default)]
pub struct SourceBook {
    pub title: String,
    pub author: String,
    pub intro: Option<String>,
    /// BCP-47 语言标签，例如 `zh` / `zh-CN`。
    pub language: String,
    /// EPUB 封面原始字节（其他格式为 `None`）。
    pub cover: Option<Vec<u8>>,
    /// TXT 实际解码用的编码名（BOM 优先于显式标签；其他格式为 `None`）。
    pub encoding: Option<String>,
    pub chapters: Vec<SourceChapter>,
}

/// 章节 = 标题 + 已归一化的纯文本段落。
#[derive(Debug, Clone, Default)]
pub struct SourceChapter {
    pub title: String,
    pub paragraphs: Vec<String>,
}

impl SourceBook {
    /// 投影成导出层 `BookDetail`；`source` 只用于缺书名时回退文件名。
    #[must_use]
    pub fn to_book(&self, source: &Path) -> BookDetail {
        let fallback = source
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("未命名");
        BookDetail {
            book_name: if self.title.trim().is_empty() {
                fallback.to_string()
            } else {
                self.title.trim().to_string()
            },
            author: if self.author.trim().is_empty() {
                "佚名".to_string()
            } else {
                self.author.trim().to_string()
            },
            intro: self.intro.clone().filter(|intro| !intro.trim().is_empty()),
            language: if self.language.trim().is_empty() {
                "zh".to_string()
            } else {
                self.language.clone()
            },
            ..BookDetail::default()
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use std::path::Path;

    use super::SourceBook;

    #[test]
    fn to_book_fills_missing_metadata() {
        let book = SourceBook::default().to_book(Path::new("本地书.txt"));
        assert_eq!(book.book_name, "本地书");
        assert_eq!(book.author, "佚名");
        assert_eq!(book.language, "zh");
        assert!(book.intro.is_none());
    }

    #[test]
    fn to_book_keeps_real_metadata() {
        let source = SourceBook {
            title: "剑来".to_string(),
            author: "烽火戏诸侯".to_string(),
            intro: Some("简介".to_string()),
            language: "zh-CN".to_string(),
            ..SourceBook::default()
        };
        let book = source.to_book(Path::new("x.epub"));
        assert_eq!(book.book_name, "剑来");
        assert_eq!(book.author, "烽火戏诸侯");
        assert_eq!(book.intro.as_deref(), Some("简介"));
        assert_eq!(book.language, "zh-CN");
    }
}
