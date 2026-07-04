// 设置页面。自动保存：字段变化即校验 + 防抖 PUT，无保存按钮。
// 只读字段（min_interval、max_interval、cf_bypass）灰显。
// 现代化布局：分区卡片（图标 + 标题 + 说明），字段左右结构，数字用 NumberField，
// 顶部轻量保存状态（保存中 / 已保存 ✓ / 保存失败）替代 toast。

import { useState, useEffect, useRef, useCallback, type ReactNode } from 'react'
import { Globe, Folder, Cloud, ArrowsRotateLeft, Gear, CircleCheck, CircleXmark } from '@gravity-ui/icons'
import { Card, Spinner } from '@heroui/react'
import { useTranslation } from 'react-i18next'
import { useSettings, useSaveSettings } from '@/hooks/use-settings'
import { languageToLocale, localeToLanguage, type Locale, type BackendLanguage } from '@/lib/language'
import AppSelect from '@/components/app-select'
import AppSwitch from '@/components/app-switch'
import NumberInput from '@/components/number-input'
import type { ExportFormat } from '@/lib/types'

/** 可编辑字段子集（PUT body 接受的字段） */
interface EditableSettings {
  download_path: string
  ext_name: ExportFormat
  txt_encoding: string
  search_filter: boolean
  proxy_enabled: boolean
  proxy_host: string
  proxy_port: number
  concurrency: number
  max_retries: number
  enable_retry: boolean
  language?: BackendLanguage
}

const FORMAT_OPTIONS: ExportFormat[] = ['epub', 'txt', 'html', 'pdf']
const LOCALES: Locale[] = ['zh-CN', 'zh-TW', 'en']
// 与后端 fields.rs::TXT_ENCODINGS 对齐。
const TXT_ENCODINGS = ['UTF-8', 'GBK', 'GB18030', 'Big5', 'BIG5HKSCS', 'UTF-16LE', 'UTF-16BE']

/** 文本框防抖保存延迟（ms）。选择/开关/数字步进立即保存，文本输入防抖。 */
const DEBOUNCE_MS = 800

/** 规范化后端返回的导出格式。后端 serde 默认序列化枚举为 PascalCase（"Epub"），
 *  这里统一转小写以匹配 AppSelect 选项 key（"epub"）。未知值回落 'epub'。 */
function normalizeFormat(raw: string | undefined): ExportFormat {
  const v = (raw ?? '').toLowerCase()
  return (FORMAT_OPTIONS as string[]).includes(v) ? (v as ExportFormat) : 'epub'
}

/** 保存状态：idle 无提示，saving 保存中，saved 已保存，error 失败。 */
type SaveState = 'idle' | 'saving' | 'saved' | 'error'

/** 字段级错误 key（i18n）。仅前端能判空的字段在此校验；目录存在性由后端返回。 */
type FieldErrors = Partial<Record<'download_path' | 'proxy_host', string>>

const inputBase =
  'w-full h-10 rounded-field border bg-field px-3 text-sm text-field-foreground outline-none transition-colors placeholder:text-field-placeholder focus:ring-2 focus:ring-focus/20 disabled:cursor-not-allowed disabled:opacity-50'

/** 分区卡片：图标徽标 + 标题 + 说明 + 内容。 */
function Section({
  icon,
  title,
  description,
  children,
}: {
  icon: ReactNode
  title: string
  description?: string
  children: ReactNode
}) {
  return (
    <Card className="p-5 sm:p-6">
      <div className="flex items-start gap-3 mb-5">
        <div className="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg bg-accent-soft text-accent-soft-foreground">
          {icon}
        </div>
        <div className="min-w-0">
          <h2 className="font-semibold leading-tight">{title}</h2>
          {description && <p className="text-xs text-default-500 mt-0.5">{description}</p>}
        </div>
      </div>
      {children}
    </Card>
  )
}

/** 字段行：标签 + 说明在左，控件在右。控件下方可显示校验错误。 */
function Field({
  label,
  description,
  error,
  children,
}: {
  label: string
  description?: string
  error?: string
  children: ReactNode
}) {
  return (
    <label className="flex flex-col gap-2 sm:flex-row sm:items-start sm:justify-between sm:gap-4">
      <div className="min-w-0 sm:pt-2">
        <span className="text-sm font-medium">{label}</span>
        {description && <p className="text-xs text-default-500 mt-0.5">{description}</p>}
      </div>
      <div className="w-full sm:w-56 sm:shrink-0">
        {children}
        {error && <p className="mt-1 text-xs text-danger">{error}</p>}
      </div>
    </label>
  )
}

/** 开关行：标签 + 说明在左，Switch 在右。 */
function ToggleRow({
  label,
  description,
  isSelected,
  onChange,
}: {
  label: string
  description?: string
  isSelected: boolean
  onChange: (v: boolean) => void
}) {
  return (
    <div className="flex items-center justify-between gap-4">
      <div className="min-w-0">
        <span className="text-sm font-medium">{label}</span>
        {description && <p className="text-xs text-default-500 mt-0.5">{description}</p>}
      </div>
      <AppSwitch isSelected={isSelected} onChange={onChange} aria-label={label} />
    </div>
  )
}

/** 顶部保存状态指示。 */
function SaveStatus({ state, t }: { state: SaveState; t: (k: string) => string }) {
  if (state === 'idle') return null
  if (state === 'saving') {
    return (
      <span className="flex items-center gap-1.5 text-sm text-default-500">
        <Spinner size="sm" /> {t('settings.status.saving')}
      </span>
    )
  }
  if (state === 'saved') {
    return (
      <span className="flex items-center gap-1.5 text-sm text-success">
        <CircleCheck width={16} height={16} /> {t('settings.status.saved')}
      </span>
    )
  }
  return (
    <span className="flex items-center gap-1.5 text-sm text-danger">
      <CircleXmark width={16} height={16} /> {t('settings.status.error')}
    </span>
  )
}

export default function SettingsPage() {
  const { data: settings, isLoading } = useSettings()
  const { mutate: save } = useSaveSettings()
  const { t, i18n } = useTranslation()
  const [form, setForm] = useState<EditableSettings | null>(null)
  const [errors, setErrors] = useState<FieldErrors>({})
  const [saveState, setSaveState] = useState<SaveState>('idle')

  // 防抖定时器与保存状态复位定时器。
  const debounceTimer = useRef<ReturnType<typeof setTimeout> | null>(null)
  const savedTimer = useRef<ReturnType<typeof setTimeout> | null>(null)

  // 仅首次加载灌入表单。后续自动保存成功会 invalidate → refetch，
  // 但不能用新数据覆盖用户正在编辑的表单，故只在 form===null 时初始化。
  useEffect(() => {
    if (!settings || form !== null) return

    const locale = languageToLocale(settings.language ?? 'SimplifiedChinese')
    if (i18n.language !== locale) {
      void i18n.changeLanguage(locale)
    }

    setForm({
      download_path: settings.download_path ?? '',
      ext_name: normalizeFormat(settings.ext_name),
      txt_encoding: settings.txt_encoding ?? 'UTF-8',
      search_filter: settings.search_filter ?? false,
      proxy_enabled: settings.proxy_enabled ?? false,
      proxy_host: settings.proxy_host ?? '',
      proxy_port: settings.proxy_port ?? 0,
      concurrency: settings.concurrency ?? 3,
      max_retries: settings.max_retries ?? 3,
      enable_retry: settings.enable_retry ?? false,
      language: settings.language ?? 'SimplifiedChinese',
    })
  }, [settings, form, i18n])

  // 卸载时清理定时器。
  useEffect(() => {
    return () => {
      if (debounceTimer.current) clearTimeout(debounceTimer.current)
      if (savedTimer.current) clearTimeout(savedTimer.current)
    }
  }, [])

  /** 前端可判定的校验（判空）。返回错误 map；空 map = 通过。目录存在性交给后端。 */
  const validate = useCallback(
    (f: EditableSettings): FieldErrors => {
      const errs: FieldErrors = {}
      if (f.download_path.trim() === '') {
        errs.download_path = t('settings.errors.pathEmpty')
      }
      if (f.proxy_enabled && f.proxy_host.trim() === '') {
        errs.proxy_host = t('settings.errors.hostEmpty')
      }
      return errs
    },
    [t],
  )

  /** 执行保存：校验 → PUT → 更新状态。后端 400 时把 download_path 错误落到字段。 */
  const commit = useCallback(
    (next: EditableSettings) => {
      const errs = validate(next)
      setErrors(errs)
      if (Object.keys(errs).length > 0) {
        setSaveState('idle')
        return
      }

      setSaveState('saving')
      save(next, {
        onSuccess: () => {
          setSaveState('saved')
          if (savedTimer.current) clearTimeout(savedTimer.current)
          savedTimer.current = setTimeout(() => setSaveState('idle'), 2000)
        },
        onError: (err) => {
          // 后端目录校验失败：download_path_empty / download_path_not_dir。
          const msg = err.message
          if (msg.includes('download_path_not_dir')) {
            setErrors((e) => ({ ...e, download_path: t('settings.errors.pathNotDir') }))
            setSaveState('idle')
          } else if (msg.includes('download_path_empty')) {
            setErrors((e) => ({ ...e, download_path: t('settings.errors.pathEmpty') }))
            setSaveState('idle')
          } else {
            setSaveState('error')
          }
        },
      })
    },
    [save, validate, t],
  )

  /**
   * 更新字段。immediate=true（选择/开关/数字步进）立即保存；
   * 否则（文本输入）防抖保存。
   */
  const update = useCallback(
    (
      key: keyof EditableSettings,
      value: string | number | boolean | BackendLanguage,
      immediate = false,
    ) => {
      setForm((prev) => {
        if (!prev) return prev
        const next = { ...prev, [key]: value }
        if (debounceTimer.current) clearTimeout(debounceTimer.current)
        if (immediate) {
          commit(next)
        } else {
          debounceTimer.current = setTimeout(() => commit(next), DEBOUNCE_MS)
        }
        return next
      })
    },
    [commit],
  )

  const handleLanguageChange = useCallback(
    (locale: Locale) => {
      void i18n.changeLanguage(locale)
      update('language', localeToLanguage(locale), true)
    },
    [i18n, update],
  )

  if (isLoading || !form) {
    return (
      <div className="space-y-4">
        <p className="text-default-500">{t('settings.loading')}</p>
      </div>
    )
  }

  return (
    <div className="mx-auto max-w-3xl">
      {/* 保存状态 */}
      <div className="mb-6 flex items-center justify-end gap-4">
        <SaveStatus state={saveState} t={t} />
      </div>

      <div className="space-y-5">
        {/* 语言 */}
        <Section icon={<Globe width={18} height={18} />} title={t('settings.language.title')}>
          <Field label={t('settings.language.label')}>
            <AppSelect
              className="w-full"
              selectedKey={i18n.language}
              onChange={(key) => handleLanguageChange(key as Locale)}
              options={LOCALES.map(locale => ({ key: locale, label: t(`settings.language.${locale}`) }))}
            />
          </Field>
        </Section>

        {/* 下载 */}
        <Section icon={<Folder width={18} height={18} />} title={t('settings.download.title')}>
          <div className="divide-y divide-separator">
            <div className="pb-4">
              <Field label={t('settings.download.path')} error={errors.download_path}>
                <input
                  className={`${inputBase} ${errors.download_path ? 'border-danger' : 'border-field-border focus:border-field-border-focus'}`}
                  value={form.download_path}
                  onChange={(e) => update('download_path', e.target.value)}
                  placeholder="./downloads"
                />
              </Field>
            </div>
            <div className="py-4">
              <Field label={t('settings.download.format')}>
                <AppSelect
                  className="w-full"
                  selectedKey={form.ext_name}
                  onChange={(key) => update('ext_name', key as ExportFormat, true)}
                  options={FORMAT_OPTIONS.map(f => ({ key: f, label: f.toUpperCase() }))}
                />
              </Field>
            </div>
            <div className="py-4">
              <Field label={t('settings.download.encoding')}>
                <AppSelect
                  className="w-full"
                  selectedKey={form.txt_encoding}
                  onChange={(key) => update('txt_encoding', key, true)}
                  options={TXT_ENCODINGS.map(e => ({ key: e, label: e }))}
                />
              </Field>
            </div>
            <div className="pt-4">
              <ToggleRow
                label={t('settings.download.searchFilter')}
                isSelected={form.search_filter}
                onChange={(v) => update('search_filter', v, true)}
              />
            </div>
          </div>
        </Section>

        {/* 代理 */}
        <Section icon={<Cloud width={18} height={18} />} title={t('settings.proxy.title')}>
          <div className="divide-y divide-separator">
            <div className="pb-4">
              <ToggleRow
                label={t('settings.proxy.enabled')}
                isSelected={form.proxy_enabled}
                onChange={(v) => update('proxy_enabled', v, true)}
              />
            </div>
            <div className="py-4">
              <Field label={t('settings.proxy.host')} error={errors.proxy_host}>
                <input
                  className={`${inputBase} ${errors.proxy_host ? 'border-danger' : 'border-field-border focus:border-field-border-focus'}`}
                  value={form.proxy_host}
                  onChange={(e) => update('proxy_host', e.target.value)}
                  disabled={!form.proxy_enabled}
                  placeholder="127.0.0.1"
                />
              </Field>
            </div>
            <div className="pt-4">
              <Field label={t('settings.proxy.port')}>
                <NumberInput
                  value={form.proxy_port}
                  onChange={(v) => update('proxy_port', v, true)}
                  minValue={0}
                  maxValue={65535}
                  isDisabled={!form.proxy_enabled}
                  aria-label={t('settings.proxy.port')}
                />
              </Field>
            </div>
          </div>
        </Section>

        {/* 重试 */}
        <Section icon={<ArrowsRotateLeft width={18} height={18} />} title={t('settings.retry.title')}>
          <div className="divide-y divide-separator">
            <div className="pb-4">
              <ToggleRow
                label={t('settings.retry.enabled')}
                isSelected={form.enable_retry}
                onChange={(v) => update('enable_retry', v, true)}
              />
            </div>
            <div className="py-4">
              <Field label={t('settings.retry.concurrency')}>
                <NumberInput
                  value={form.concurrency}
                  onChange={(v) => update('concurrency', v, true)}
                  minValue={1}
                  maxValue={64}
                  aria-label={t('settings.retry.concurrency')}
                />
              </Field>
            </div>
            <div className="pt-4">
              <Field label={t('settings.retry.maxRetries')}>
                <NumberInput
                  value={form.max_retries}
                  onChange={(v) => update('max_retries', v, true)}
                  minValue={0}
                  maxValue={20}
                  isDisabled={!form.enable_retry}
                  aria-label={t('settings.retry.maxRetries')}
                />
              </Field>
            </div>
          </div>
        </Section>

        {/* 只读 */}
        <Section
          icon={<Gear width={18} height={18} />}
          title={t('settings.readonly.title')}
        >
          <div className="grid grid-cols-1 sm:grid-cols-3 gap-4 text-sm">
            <div className="rounded-lg bg-default-100/50 px-4 py-3">
              <p className="text-xs text-default-500">{t('settings.readonly.minInterval')}</p>
              <p className="mt-1 font-medium tabular-nums">{settings?.min_interval ?? '-'}</p>
            </div>
            <div className="rounded-lg bg-default-100/50 px-4 py-3">
              <p className="text-xs text-default-500">{t('settings.readonly.maxInterval')}</p>
              <p className="mt-1 font-medium tabular-nums">{settings?.max_interval ?? '-'}</p>
            </div>
            <div className="rounded-lg bg-default-100/50 px-4 py-3">
              <p className="text-xs text-default-500">{t('settings.readonly.cfBypass')}</p>
              <p className="mt-1 font-medium truncate">{settings?.cf_bypass || '-'}</p>
            </div>
          </div>
        </Section>
      </div>
    </div>
  )
}
