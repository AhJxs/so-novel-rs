import i18n from 'i18next'
import { initReactI18next } from 'react-i18next'
import zhCN from './locales/zh-CN.json'
import zhTW from './locales/zh-TW.json'
import en from './locales/en.json'

i18n
  .use(initReactI18next)
  .init({
    resources: {
      'zh-CN': { translation: zhCN },
      'zh-TW': { translation: zhTW },
      en: { translation: en },
    },
    lng: 'zh-CN', // 默认语言，会被后端设置覆盖
    fallbackLng: 'en',
    interpolation: {
      escapeValue: false,
    },
  })

export default i18n
