// 书源管理页面。顶部「全部测速」一键测试所有书源；每行右侧 Switch 管理启停。

import { Thunderbolt } from '@gravity-ui/icons'
import { Card, Button, Chip, Spinner } from '@heroui/react'
import { useSources, useToggleSource, useTestSource } from '@/hooks/use-sources'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import AppSwitch from '@/components/app-switch'

type TestResult = { ok: boolean; latency_ms: number } | 'testing'

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
    setResults(Object.fromEntries(sources.map(s => [s.id, 'testing' as const])))
    await Promise.all(
      sources.map(async (s) => {
        try {
          const res = await testFn(s.id)
          setResults(r => ({ ...r, [s.id]: res }))
        } catch {
          setResults(r => ({ ...r, [s.id]: { ok: false, latency_ms: 0 } }))
        } finally {
          setTestedCount(c => c + 1)
        }
      }),
    )
    setTestingAll(false)
  }

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between gap-3">
        {/* 左侧：启用 / 未启用 chip（颜色对照 tasks 页的 STATUS_CHIP_BG，
            enabled=绿 / disabled=灰，跟"行"启用状态视觉一致）。零计数不渲染
            —— 跟 tasks 页同样的「少即是多」策略。 */}
        <div className="flex items-center gap-2 flex-wrap">
          {(() => {
            const enabled = sources.filter(s => s.enabled).length
            const disabled = sources.length - enabled
            return (
              <>
                {enabled > 0 && (
                  <Chip size="md" variant="soft" className="text-green-500 bg-green-500/15">
                    {t('sources.enabledLabel')} · {enabled}
                  </Chip>
                )}
                {disabled > 0 && (
                  <Chip size="md" variant="soft" className="text-gray-400 bg-gray-500/15">
                    {t('sources.disabledLabel')} · {disabled}
                  </Chip>
                )}
              </>
            )
          })()}
        </div>
        <Button variant="primary" size="sm" isDisabled={testingAll || sources.length === 0} onPress={testAll}>
          {testingAll ? <Spinner size="sm" /> : <Thunderbolt />}
          {testingAll ? t('sources.testingAll', { done: testedCount, total: sources.length }) : t('sources.testAll')}
        </Button>
      </div>
      <div className="space-y-2">
        {sources.map(s => {
          const result = results[s.id]
          return (
            <Card key={s.id} className={`px-5 py-3.5 transition-opacity ${!s.enabled ? 'opacity-60' : ''}`}>
              <div className="flex items-center gap-4">
                <div className={`w-2.5 h-2.5 rounded-full flex-shrink-0 ${s.enabled ? 'bg-green-500' : 'bg-default'}`} />
                <div className="flex-1 min-w-0">
                  <p className="text-sm font-medium truncate">{s.name}</p>
                  <p className="text-xs text-default-500 truncate">{s.url}</p>
                </div>
                {result === 'testing' && <Spinner size="sm" />}
                {result && result !== 'testing' && (
                  <Chip size="sm" variant="soft" className={result.ok ? 'text-success' : 'text-danger'}>
                    {result.ok ? `${result.latency_ms}ms` : t('sources.timeout')}
                  </Chip>
                )}
                <AppSwitch isSelected={s.enabled} onChange={() => toggle(s.id)} aria-label={s.name} />
              </div>
            </Card>
          )
        })}
      </div>
    </div>
  )
}
