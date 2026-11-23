//! 主题（深色 / 浅色 / 跟随系统）的唯一入口。
//!
//! - 偏好持久化在 localStorage `hmi.theme`（index.html 的首帧脚本读同一个键防闪烁）。
//! - 实际生效主题写到 `<html data-theme="...">`，tokens.css 按它切换整套变量。
//! - Naive UI 的 themeOverrides 从 CSS 变量实时读取，不再手工维护第二份调色板。

import { computed, ref, watchEffect } from 'vue'
import { darkTheme, type GlobalThemeOverrides } from 'naive-ui'

export type ThemeMode = 'dark' | 'light' | 'system'

const STORAGE_KEY = 'hmi.theme'

function loadMode(): ThemeMode {
  try {
    const raw = localStorage.getItem(STORAGE_KEY)
    return raw === 'dark' || raw === 'system' ? raw : 'light'
  } catch {
    return 'light'
  }
}

export const themeMode = ref<ThemeMode>(loadMode())

const systemDark = ref(true)

function trackSystem(): void {
  const mql = window.matchMedia('(prefers-color-scheme: dark)')
  systemDark.value = mql.matches
  mql.addEventListener('change', (e) => {
    systemDark.value = e.matches
  })
}

export const isDark = computed(
  () => themeMode.value === 'dark' || (themeMode.value === 'system' && systemDark.value),
)

export function setThemeMode(mode: ThemeMode): void {
  themeMode.value = mode
  try {
    localStorage.setItem(STORAGE_KEY, mode)
  } catch {
    /* 隐私模式下忽略持久化失败 */
  }
}

/** 主题循环切换：深色 → 浅色 → 跟随系统。 */
export function cycleThemeMode(): ThemeMode {
  const next: ThemeMode = themeMode.value === 'dark' ? 'light' : themeMode.value === 'light' ? 'system' : 'dark'
  setThemeMode(next)
  return next
}

function cssVar(name: string, fallback = ''): string {
  const v = getComputedStyle(document.documentElement).getPropertyValue(name).trim()
  return v || fallback
}

function themeColor(name: string): string {
  // Naive UI 的颜色解析器接受逗号分隔的 RGB。用浏览器序列化 token。
  const { style } = document.createElement('span')
  style.color = cssVar(name)
  return style.color
}

/** 从 tokens.css 读变量生成 Naive UI 覆盖（调用前 data-theme 必须已就位）。 */
function buildNaiveOverrides(): GlobalThemeOverrides {
  const accent = themeColor('--accent')
  const onAccent = themeColor('--on-accent')
  return {
    common: {
      primaryColor: accent,
      primaryColorHover: themeColor('--accent-hover'),
      primaryColorPressed: themeColor('--accent-active'),
      primaryColorSuppl: accent,
      infoColor: accent,
      successColor: themeColor('--good'),
      warningColor: themeColor('--warn'),
      errorColor: themeColor('--bad'),
      bodyColor: cssVar('--bg-0', '#0a0d12'),
      cardColor: cssVar('--bg-1', '#0f141b'),
      modalColor: cssVar('--bg-popover', '#10161f'),
      popoverColor: cssVar('--bg-popover', '#10161f'),
      tableColor: cssVar('--bg-1', '#0f141b'),
      inputColor: cssVar('--bg-0', '#0a0d12'),
      borderColor: cssVar('--line', '#26313f'),
      dividerColor: cssVar('--line-soft', '#1c2531'),
      textColorBase: cssVar('--text-1', '#e8eef7'),
      textColor1: cssVar('--text-1', '#e8eef7'),
      textColor2: cssVar('--text-2', '#c2cedd'),
      textColor3: cssVar('--text-3', '#8494a8'),
      textColorDisabled: cssVar('--text-disabled', '#5a6879'),
      borderRadius: cssVar('--radius-sm', '8px'),
      borderRadiusSmall: cssVar('--radius-xs', '6px'),
      fontFamily: cssVar('--font-sans', 'Inter, "Segoe UI", sans-serif'),
      fontFamilyMono: cssVar('--font-mono', 'ui-monospace, Consolas, monospace'),
      fontSize: '13px',
      fontSizeSmall: '12px',
      fontSizeMedium: '14px',
    },
    Button: {
      textColorPrimary: onAccent,
      textColorHoverPrimary: onAccent,
      textColorPressedPrimary: onAccent,
      textColorFocusPrimary: onAccent,
    },
  }
}

export const themeOverrides = ref<GlobalThemeOverrides>({})

// dataset 改动后 getComputedStyle 会同步强制重算，所以这里能读到切换后的变量。
watchEffect(() => {
  document.documentElement.dataset.theme = isDark.value ? 'dark' : 'light'
  themeOverrides.value = buildNaiveOverrides()
})

trackSystem()

/** Naive UI 的主题对象：深色用 darkTheme，浅色用 null（Naive 默认即浅色）。 */
export function naiveTheme(dark: boolean): typeof darkTheme | null {
  return dark ? darkTheme : null
}
