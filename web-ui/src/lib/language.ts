// 后端 Language enum 的字符串表示（来自 Settings.language）
export type BackendLanguage = 'SimplifiedChinese' | 'TraditionalChinese' | 'English'

// 前端 i18n locale
export type Locale = 'zh-CN' | 'zh-TW' | 'en'

/**
 * 后端 Language enum 映射到前端 i18n locale
 */
export function languageToLocale(lang: BackendLanguage): Locale {
  const map: Record<BackendLanguage, Locale> = {
    SimplifiedChinese: 'zh-CN',
    TraditionalChinese: 'zh-TW',
    English: 'en',
  }
  return map[lang] || 'en'
}

/**
 * 前端 i18n locale 映射回后端 Language enum
 */
export function localeToLanguage(locale: Locale): BackendLanguage {
  const map: Record<Locale, BackendLanguage> = {
    'zh-CN': 'SimplifiedChinese',
    'zh-TW': 'TraditionalChinese',
    en: 'English',
  }
  return map[locale] || 'English'
}
