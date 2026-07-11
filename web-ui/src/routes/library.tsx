// 书库页面。按扩展名过滤（前端过滤，后端暂不支持 ?ext=）。
// 提供下载链接（GET /api/files/:filename）+ 删除（带确认对话框）+ 分页（HeroUI Pagination）。
// 过滤 tab 右侧挂 Badge 显示每种类型文件数，替代之前的「共 N 个文件」页头文本 ——
// 视觉重心下移到 tab 本身，扫一眼就知道「EPUB 8 个、PDF 0 个」该不该换 tab。

import { Book, ArrowDown, TrashBin } from "@gravity-ui/icons";
import { Card, Button, Tabs, Skeleton, Pagination, Badge } from "@heroui/react";
import { toast } from "sonner";
import { useState, useEffect, useCallback, useMemo } from "react";
import { useLibrary, useDeleteFile } from "@/hooks/use-library";
import { formatBytes, formatUnixDate } from "@/lib/utils";
import { useTranslation } from "react-i18next";
import ConfirmDialog from "@/components/confirm-dialog";
import type { LibraryFile } from "@/lib/types";

const EXT_COLOR: Record<string, string> = {
  epub: "bg-green-500",
  pdf: "bg-red-500",
  txt: "bg-blue-500",
  html: "bg-orange-500",
  md: "bg-purple-500",
};

// tab Badge 颜色：epub/pdf/txt/html/md 映射 HeroUI 语义色（success / danger /
// accent / warning / default），跟卡片左侧 EXT_COLOR 颜色块视觉对齐；all 用中性 default。
// md 没有专属语义色，沿用 default（neutral）—— 5 种格式已占满 success/danger/accent/warning。
// HeroUI v3 Badge 单独使用时是 inline 元素（不像 v2 那样 absolute 角标），
// 因此可以直接挂在 tab label 后面作为内联数字徽章。
const TAB_BADGE_COLOR: Record<string, "default" | "success" | "danger" | "accent" | "warning"> = {
  all: "default",
  epub: "success",
  pdf: "danger",
  txt: "accent",
  html: "warning",
  md: "default",
};

const PAGE_SIZE = 12;

export default function LibraryPage() {
  const [ext, setExt] = useState<string>("all");
  const [page, setPage] = useState(1);
  const [pending, setPending] = useState<LibraryFile | null>(null); // 待删除文件（打开确认框）
  const { data: allFiles = [], isLoading } = useLibrary();
  const { mutate: del } = useDeleteFile();
  const { t } = useTranslation();

  const files =
    ext === "all" ? allFiles : allFiles.filter((f) => f.ext === ext);
  const totalPages = Math.max(1, Math.ceil(files.length / PAGE_SIZE));
  const paged = files.slice((page - 1) * PAGE_SIZE, page * PAGE_SIZE);

  // 每种 ext 的文件数（含 0）—— 用在 Tabs.Tab 的 Badge 上。
  // 一次 reduce 算 6 个数（O(n)）而不是每次 tab 渲染时 filter（O(n) × 6）。
  const extCounts = useMemo(() => {
    const counts: Record<string, number> = { all: allFiles.length, epub: 0, txt: 0, pdf: 0, html: 0, md: 0 }
    for (const f of allFiles) {
      if (f.ext in counts) counts[f.ext]++
    }
    return counts
  }, [allFiles])

  // 切换过滤或文件数变化时，把页码夹回合法范围。
  useEffect(() => {
    setPage((p) => Math.min(p, totalPages));
  }, [totalPages]);

  // 总页数多时折叠中间页：始终保留首页 / 末页 + 当前页前后各 1 页，中间用 … 代替。
  const pageItems = useCallback((): ("ellipsis" | number)[] => {
    if (totalPages <= 7)
      return Array.from({ length: totalPages }, (_, i) => i + 1);
    const items: ("ellipsis" | number)[] = [1];
    if (page > 3) items.push("ellipsis");
    const start = Math.max(2, page - 1);
    const end = Math.min(totalPages - 1, page + 1);
    for (let i = start; i <= end; i++) items.push(i);
    if (page < totalPages - 2) items.push("ellipsis");
    items.push(totalPages);
    return items;
  }, [page, totalPages]);

  const handleFilterChange = (key: string) => {
    setExt(key);
    setPage(1);
  };

  const confirmDelete = () => {
    if (!pending) return;
    del(pending.filename, {
      onSuccess: () => toast.success(t("library.deleted")),
    });
    setPending(null);
  };

  return (
    <div className="space-y-4">
      <Tabs
        selectedKey={ext}
        onSelectionChange={(key) => handleFilterChange(String(key))}
      >
        <Tabs.ListContainer>
          <Tabs.List aria-label="library-filter">
            {["all", "epub", "txt", "pdf", "html", "md"].map((tab) => (
              <Tabs.Tab key={tab} id={tab}>
                {t(`library.filter.${tab}`).toUpperCase()}
                <Badge size="sm" color={TAB_BADGE_COLOR[tab]}>
                  {extCounts[tab] ?? 0}
                </Badge>
                <Tabs.Indicator />
              </Tabs.Tab>
            ))}
          </Tabs.List>
        </Tabs.ListContainer>
      </Tabs>

      {isLoading && (
        <div className="space-y-2">
          {Array.from({ length: 4 }).map((_, i) => (
            <Skeleton key={i} className="h-16 rounded-xl" />
          ))}
        </div>
      )}

      {!isLoading && files.length === 0 && (
        <div className="text-center py-16 text-default-500">
          <Book className="w-12 h-12 mx-auto mb-3 opacity-40" />
          <p>{t("library.empty")}</p>
        </div>
      )}

      <div className="space-y-2">
        {paged.map((f) => (
          <Card key={f.filename} className="px-5 py-3.5 group">
            <div className="flex items-center gap-4">
              <div
                className={`w-10 h-10 rounded-lg ${EXT_COLOR[f.ext] ?? "bg-default"} flex items-center justify-center text-white text-xs font-bold`}
              >
                {f.ext.toUpperCase()}
              </div>
              <div className="flex-1 min-w-0">
                <p className="text-sm font-medium truncate">{f.filename}</p>
                <p className="text-xs text-default-500">
                  {formatBytes(f.size)} · {formatUnixDate(f.modified)}
                </p>
              </div>
              <div className="flex gap-2">
                <a
                  href={`/api/files/${encodeURIComponent(f.filename)}`}
                  download
                >
                  <Button size="sm">
                    <ArrowDown />
                    {t("library.download")}
                  </Button>
                </a>
                <Button
                  variant="danger"
                  size="sm"
                  onPress={() => setPending(f)}
                >
                  <TrashBin />
                  {t("library.delete")}
                </Button>
              </div>
            </div>
          </Card>
        ))}
      </div>

      {/* 分页 */}
      {!isLoading && totalPages > 1 && (
        <div className="pt-2">
          <Pagination className="justify-end">
            <Pagination.Content>
              <Pagination.Item>
                <Pagination.Previous
                  isDisabled={page === 1}
                  onPress={() => setPage((p) => Math.max(1, p - 1))}
                >
                  <Pagination.PreviousIcon />
                </Pagination.Previous>
              </Pagination.Item>
              {pageItems().map((n, i) =>
                n === "ellipsis" ? (
                  <Pagination.Item key={`e-${i}`}>
                    <Pagination.Ellipsis />
                  </Pagination.Item>
                ) : (
                  <Pagination.Item key={n}>
                    <Pagination.Link
                      isActive={n === page}
                      onPress={() => setPage(n)}
                    >
                      {n}
                    </Pagination.Link>
                  </Pagination.Item>
                ),
              )}
              <Pagination.Item>
                <Pagination.Next
                  isDisabled={page === totalPages}
                  onPress={() => setPage((p) => Math.min(totalPages, p + 1))}
                >
                  <Pagination.NextIcon />
                </Pagination.Next>
              </Pagination.Item>
            </Pagination.Content>
          </Pagination>
        </div>
      )}

      {/* 删除确认 */}
      <ConfirmDialog
        isOpen={pending !== null}
        onOpenChange={(open) => {
          if (!open) setPending(null);
        }}
        title={t("library.deleteConfirm.title")}
        message={t("library.deleteConfirm.message", {
          name: pending?.filename ?? "",
        })}
        confirmLabel={t("library.deleteConfirm.confirm")}
        cancelLabel={t("library.deleteConfirm.cancel")}
        onConfirm={confirmDelete}
      />
    </div>
  );
}
