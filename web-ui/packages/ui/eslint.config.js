import js from '@eslint/js'
import globals from 'globals'
import reactHooks from 'eslint-plugin-react-hooks'
import reactRefresh from 'eslint-plugin-react-refresh'
import tseslint from 'typescript-eslint'
import { defineConfig, globalIgnores } from 'eslint/config'

export default defineConfig([
  globalIgnores(['dist']),
  {
    files: ['**/*.{ts,tsx}'],
    extends: [
      js.configs.recommended,
      tseslint.configs.recommended,
      reactHooks.configs.flat.recommended,
      reactRefresh.configs.vite,
    ],
    languageOptions: {
      globals: globals.browser,
    },
    rules: {
      // shadcn 生成组件导出 variants 常量是固有模式（buttonVariants 等）
      'react-refresh/only-export-components': ['warn', { allowConstantExport: true }],
      // use-mobile（matchMedia 断点 hook）在 effect 里同步 setState 是 shadcn 标准实现
      'react-hooks/set-state-in-effect': 'off',
    },
  },
])
