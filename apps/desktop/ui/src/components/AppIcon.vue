<script setup lang="ts">
import { computed } from 'vue'

const props = withDefaults(defineProps<{ name: string; size?: number }>(), { size: 18 })

// 统一线性图标（stroke），少量实心图标单独标记。
const PATHS: Record<string, string[]> = {
  pulse: ['M3 12h4l2.5-7 4 14L16 12h5'],
  plug: ['M9 2v6', 'M15 2v6', 'M7 8h10v3a5 5 0 0 1-10 0V8Z', 'M12 16v6'],
  chip: [
    'M7 7h10v10H7z',
    'M9 2v3',
    'M15 2v3',
    'M9 19v3',
    'M15 19v3',
    'M2 9h3',
    'M2 15h3',
    'M19 9h3',
    'M19 15h3',
  ],
  grid: ['M3 3h8v8H3z', 'M13 3h8v8h-8z', 'M3 13h8v8H3z', 'M13 13h8v8h-8z'],
  refresh: ['M21 12a9 9 0 1 1-3-6.7', 'M21 3v6h-6'],
  plus: ['M12 5v14', 'M5 12h14'],
  scan: [
    'M3 7V5a2 2 0 0 1 2-2h2',
    'M17 3h2a2 2 0 0 1 2 2v2',
    'M21 17v2a2 2 0 0 1-2 2h-2',
    'M7 21H5a2 2 0 0 1-2-2v-2',
    'M3 12h18',
  ],
  trash: ['M3 6h18', 'M8 6V4h8v2', 'M19 6l-1 14H6L5 6', 'M10 11v6', 'M14 11v6'],
  edit: ['M12 20h9', 'M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4Z'],
  link: [
    'M10 13a5 5 0 0 0 7 0l3-3a5 5 0 0 0-7-7l-1 1',
    'M14 11a5 5 0 0 0-7 0l-3 3a5 5 0 0 0 7 7l1-1',
  ],
  arrowRight: ['M5 12h14', 'M13 6l6 6-6 6'],
  x: ['M6 6l12 12', 'M18 6L6 18'],
  eye: ['M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7S2 12 2 12z', 'M12 9a3 3 0 1 0 0 6 3 3 0 0 0 0-6z'],
  alert: [
    'M12 9v4',
    'M12 17h.01',
    'M10.3 3.9 1.8 18a2 2 0 0 0 1.7 3h17a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 0-3.4 0z',
  ],
  check: ['M20 6 9 17l-5-5'],
  chevronDown: ['M6 9l6 6 6-6'],
  download: ['M12 3v12', 'M7 10l5 5 5-5', 'M5 21h14'],
  activity: ['M22 12h-4l-3 9L9 3l-3 9H2'],
  power: ['M12 2v10', 'M18.4 6.6a9 9 0 1 1-12.8 0'],
  clock: ['M12 8v4l3 2', 'M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18z'],
  dots: ['M5 12h.01', 'M12 12h.01', 'M19 12h.01'],
  layout: ['M3 3h18v18H3z', 'M3 9h18', 'M9 9v12'],
  package: ['M21 8 12 3 3 8v8l9 5 9-5V8Z', 'M3 8l9 5 9-5', 'M12 13v8'],
  folder: ['M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V7Z'],
  plugin: [
    'M9 3v4',
    'M15 3v4',
    'M7 7h10v4.5a5 5 0 0 1-10 0V7Z',
    'M12 16.5V21',
    'M9 21h6',
  ],
}

const FILLED = new Set(['play', 'dots'])

const paths = computed(() => PATHS[props.name] ?? PATHS.dots)
const filled = computed(() => FILLED.has(props.name))
</script>

<template>
  <svg
    :width="size"
    :height="size"
    viewBox="0 0 24 24"
    :fill="filled ? 'currentColor' : 'none'"
    stroke="currentColor"
    :stroke-width="filled ? 0 : 1.7"
    stroke-linecap="round"
    stroke-linejoin="round"
    aria-hidden="true"
  >
    <path v-for="(d, i) in paths" :key="i" :d="d" />
  </svg>
</template>
