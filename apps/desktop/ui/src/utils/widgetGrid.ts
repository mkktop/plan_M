import type { WidgetRect } from '@/api/types'

export const WIDGET_COLUMNS = 12
export const WIDGET_ROWS = 200

export function widgetsOverlap(a: WidgetRect, b: WidgetRect): boolean {
  return a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h
}

export function nextWidgetRect(rects: WidgetRect[], size: Pick<WidgetRect, 'w' | 'h'>): WidgetRect | null {
  for (let y = 0; y + size.h <= WIDGET_ROWS; y += 1) {
    for (let x = 0; x + size.w <= WIDGET_COLUMNS; x += 1) {
      const candidate = { x, y, w: size.w, h: size.h }
      if (!rects.some((rect) => widgetsOverlap(rect, candidate))) return candidate
    }
  }
  return null
}
