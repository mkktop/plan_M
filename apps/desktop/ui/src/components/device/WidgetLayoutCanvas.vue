<script setup lang="ts">
import { computed, ref } from 'vue'
import AppIcon from '@/components/AppIcon.vue'
import { componentLabel } from '@/utils/devicePages'
import { WIDGET_COLUMNS, WIDGET_ROWS, widgetsOverlap } from '@/utils/widgetGrid'
import type { WidgetRect } from '@/api/types'
import type { DeviceWidgetOption } from '@/plugins/deviceWidgets'

const props = defineProps<{
  widgets: { kind: string; title: string; rect: WidgetRect | null }[]
  options: DeviceWidgetOption[]
}>()
const emit = defineEmits<{
  move: [index: number, position: WidgetRect]
  select: [index: number]
}>()
const board = ref<HTMLElement>()
const rowHeight = 24
const drag = ref<{ index: number; dx: number; dy: number }>()
const resize = ref<{ index: number; pointerId: number; x: number; y: number; rect: WidgetRect }>()
const preview = ref<WidgetRect>()
const knownWidgets = computed(() => new Map(props.options.map((widget) => [widget.kind, widget])))
const rowCount = computed(() => Math.min(WIDGET_ROWS, Math.max(4, ...props.widgets.map(({ rect }) =>
  rect ? rect.y + rect.h + 2 : 0,
), preview.value ? preview.value.y + preview.value.h + 2 : 0)))
const blocked = computed(() => {
  const target = preview.value
  return target && collides(resize.value?.index ?? drag.value?.index, target)
})

function collides(index: number | undefined, target: WidgetRect): boolean {
  return props.widgets.some(({ rect }, i) => rect && i !== index && widgetsOverlap(rect, target))
}

function rectStyle(rect: WidgetRect) {
  return {
    left: `${rect.x / WIDGET_COLUMNS * 100}%`,
    top: `${rect.y * rowHeight}px`,
    width: `${rect.w / WIDGET_COLUMNS * 100}%`,
    height: `${rect.h * rowHeight}px`,
  }
}

function startsDrag(event: DragEvent, index: number): void {
  const bounds = (event.currentTarget as HTMLElement).getBoundingClientRect()
  drag.value = { index, dx: event.clientX - bounds.left, dy: event.clientY - bounds.top }
  event.dataTransfer?.setData('text/plain', String(index))
  if (event.dataTransfer) event.dataTransfer.effectAllowed = 'move'
}

function dragOver(event: DragEvent): void {
  const active = drag.value
  const bounds = board.value?.getBoundingClientRect()
  const rect = active && props.widgets[active.index]?.rect
  if (!active || !bounds || !rect) return
  event.preventDefault()
  const x = Math.round((event.clientX - bounds.left - active.dx) / bounds.width * WIDGET_COLUMNS)
  const y = Math.round((event.clientY - bounds.top - active.dy) / rowHeight)
  preview.value = {
    ...rect,
    x: Math.max(0, Math.min(WIDGET_COLUMNS - rect.w, x)),
    y: Math.max(0, Math.min(WIDGET_ROWS - rect.h, y)),
  }
  if (event.dataTransfer) event.dataTransfer.dropEffect = blocked.value ? 'none' : 'move'
}

function clearDrag(): void {
  drag.value = undefined
  preview.value = undefined
}

function drop(event: DragEvent): void {
  event.preventDefault()
  if (drag.value && preview.value && !blocked.value) emit('move', drag.value.index, preview.value)
  clearDrag()
}

function resizedRect(index: number, rect: WidgetRect, dw: number, dh: number): WidgetRect {
  const limits = knownWidgets.value.get(props.widgets[index].kind)?.external?.size
  return { ...rect,
    w: Math.min(WIDGET_COLUMNS - rect.x, Math.max(limits?.minW ?? 1, rect.w + dw)),
    h: Math.min(WIDGET_ROWS - rect.y, Math.max(limits?.minH ?? 1, rect.h + dh)),
  }
}

function startResize(event: PointerEvent, index: number, rect: WidgetRect): void {
  if (event.button !== 0) return
  const handle = event.currentTarget as HTMLElement
  handle.setPointerCapture(event.pointerId)
  resize.value = { index, pointerId: event.pointerId, x: event.clientX, y: event.clientY, rect: { ...rect } }
}

function moveResize(event: PointerEvent): void {
  const active = resize.value
  const width = board.value?.getBoundingClientRect().width
  if (!active || active.pointerId !== event.pointerId || !width) return
  const dw = Math.round((event.clientX - active.x) * WIDGET_COLUMNS / width)
  const dh = Math.round((event.clientY - active.y) / rowHeight)
  preview.value = resizedRect(active.index, active.rect, dw, dh)
}

function finishResize(event: PointerEvent, commit: boolean): void {
  const active = resize.value
  if (!active || active.pointerId !== event.pointerId) return
  if (commit && preview.value && !blocked.value) emit('move', active.index, preview.value)
  resize.value = undefined
  preview.value = undefined
}

function resizeByKey(event: KeyboardEvent, index: number, rect: WidgetRect): void {
  const directions: Record<string, [number, number]> = { ArrowLeft: [-1, 0], ArrowRight: [1, 0], ArrowUp: [0, -1], ArrowDown: [0, 1] }
  const direction = directions[event.key]
  if (!direction) return
  event.preventDefault()
  const target = resizedRect(index, rect, ...direction)
  if (!collides(index, target)) emit('move', index, target)
}

function titleOf(widget: { kind: string; title: string }): string {
  return widget.title || knownWidgets.value.get(widget.kind)?.title || componentLabel(widget.kind)
}
</script>

<template>
  <section class="widget-board">
    <header><strong>布局画布</strong><small>拖动组件调整位置，拖动右下角调整大小</small></header>
    <div class="widget-board-scroll">
      <div ref="board" class="widget-board-grid" :style="{ height: `${rowCount * rowHeight}px` }"
        @dragover="dragOver" @drop="drop">
        <template v-for="(widget, index) in widgets" :key="index">
          <button v-if="widget.rect" type="button" class="widget-board-tile"
            :class="{ moving: drag?.index === index }" :style="rectStyle(widget.rect)"
            :title="titleOf(widget)" draggable="true"
            @dragstart="startsDrag($event, index)" @dragend="clearDrag" @click="emit('select', index)">
            <AppIcon :name="knownWidgets.get(widget.kind)?.icon || 'plugin'" :size="15" />
            <span>{{ titleOf(widget) }}</span>
          </button>
          <button v-if="widget.rect" type="button" class="widget-board-resize"
            :style="{ left: `${(widget.rect.x + widget.rect.w) / WIDGET_COLUMNS * 100}%`, top: `${(widget.rect.y + widget.rect.h) * rowHeight}px` }"
            :aria-label="`调整${titleOf(widget)}大小`" title="拖动或用方向键调整大小"
            @pointerdown="startResize($event, index, widget.rect)" @pointermove="moveResize"
            @pointerup="finishResize($event, true)" @lostpointercapture="finishResize($event, false)"
            @keydown="resizeByKey($event, index, widget.rect)" />
        </template>
        <div v-if="preview" class="widget-board-target" :class="{ blocked }" :style="rectStyle(preview)" />
      </div>
    </div>
  </section>
</template>
