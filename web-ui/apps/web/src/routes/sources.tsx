// 书源管理页面。顶部「全部测速」一键测试所有书源；每行右侧 Switch 管理启停。

import { Loader2, Zap } from "lucide-react"
import { Badge } from "@workspace/ui/components/badge"
import { Button } from "@workspace/ui/components/button"
import { Card, CardContent } from "@workspace/ui/components/card"
import { Switch } from "@workspace/ui/components/switch"
import { useSources, useToggleSource, useTestSource } from "@/hooks/use-sources"
import { useState } from "react"
import { useTranslation } from "react-i18next"

type TestResult = { ok: boolean; latency_ms: number } | "testing"

export default function SourcesPage() {
  const { data: sources = [] } = useSources()
  const { mutate: toggle } = useToggleSource()
  const { mutateAsync: testFn } = useTestSource()
  const { t } = useTranslation()
  const [results, setResults] = useState<Record<number, TestResult>>({})
  const [testingAll, setTestingAll] = useState(false)
  const [testedCount, setTestedCount] = useState(0)

  // 一键测速：并发测所有书源，各自结果独立回填，完成一个计数 +1 反映进度。
  const testAll = async () => {
    if (testingAll) return
    setTestingAll(true)
    setTestedCount(0)
    setResults(Object.fromEntries(sources.map((s) => [s.id, "testing" as const])))
    await Promise.all(
      sources.map(async (s) => {
        try {
          const res = await testFn(s.id)
          setResults((r) => ({ ...r, [s.id]: res }))
        } catch {
          setResults((r) => ({ ...r, [s.id]: { ok: false, latency_ms: 0 } }))
        } finally {
          setTestedCount((c) => c + 1)
        }
      }),
    )
    setTestingAll(false)
  }

  return (
    <div className="flex flex-col gap-4">
      <div className="flex items-center justify-between gap-3">
        {/* 左侧：启用 / 未启用计数 badge */}
        <div className="flex flex-wrap items-center gap-2">
          {(() => {
            const enabled = sources.filter((s) => s.enabled).length
            const disabled = sources.length - enabled
            return (
              <>
                {enabled > 0 && (
                  <Badge className="bg-green-500/15 text-green-500">
                    {t("sources.enabledLabel")} · {enabled}
                  </Badge>
                )}
                {disabled > 0 && (
                  <Badge className="bg-gray-500/15 text-gray-400">
                    {t("sources.disabledLabel")} · {disabled}
                  </Badge>
                )}
              </>
            )
          })()}
        </div>
        <Button size="sm" disabled={testingAll || sources.length === 0} onClick={testAll}>
          {testingAll ? <Loader2 className="animate-spin" /> : <Zap data-icon="inline-start" />}
          {testingAll
            ? t("sources.testingAll", { done: testedCount, total: sources.length })
            : t("sources.testAll")}
        </Button>
      </div>
      <div className="flex flex-col gap-2">
        {sources.map((s) => {
          const result = results[s.id]
          return (
            <Card key={s.id} className={`transition-opacity ${!s.enabled ? "opacity-60" : ""}`}>
              <CardContent className="flex items-center gap-4 p-4">
                <div
                  className={`size-2.5 flex-shrink-0 rounded-full ${s.enabled ? "bg-green-500" : "bg-muted-foreground/40"}`}
                />
                <div className="min-w-0 flex-1">
                  <p className="truncate text-sm font-medium">{s.name}</p>
                  <p className="truncate text-xs text-muted-foreground">{s.url}</p>
                </div>
                {result === "testing" && <Loader2 className="size-4 animate-spin text-muted-foreground" />}
                {result && result !== "testing" && (
                  <Badge variant="secondary" className={result.ok ? "text-green-600" : "text-destructive"}>
                    {result.ok ? `${result.latency_ms}ms` : t("sources.timeout")}
                  </Badge>
                )}
                <Switch checked={s.enabled} onCheckedChange={() => toggle(s.id)} aria-label={s.name} />
              </CardContent>
            </Card>
          )
        })}
      </div>
    </div>
  )
}
