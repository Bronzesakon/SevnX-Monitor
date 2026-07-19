import type { Decimal, DisplayValue, Maybe } from '../types/contracts'

export const EMPTY_VALUE = '--'

export function hasValue(value: Maybe<unknown>): value is string | number {
  return value !== null && value !== undefined && value !== ''
}

export function plainValue(value: Maybe<Decimal>, fallback = EMPTY_VALUE): string {
  return hasValue(value) ? String(value) : fallback
}

export function compactTokenValue(value: Maybe<Decimal>, fallback = EMPTY_VALUE): string {
  if (!hasValue(value)) return fallback
  const total = Number(value)
  if (!Number.isFinite(total) || total < 1_000_000) return String(value)
  return `${(total / 1_000_000).toFixed(2)}M`
}

export function tokenValue(value: Maybe<Decimal | string>, fallback = EMPTY_VALUE): string {
  if (!hasValue(value)) return fallback
  const text = String(value).replaceAll(',', '')
  if (!/^\d+$/.test(text)) return String(value)
  return new Intl.NumberFormat('en-US').format(Number(text))
}

export function displayValue(value: Maybe<DisplayValue>, fallback = EMPTY_VALUE): string {
  if (!value) return fallback
  return hasValue(value.display) ? String(value.display) : plainValue(value.value, fallback)
}

export function moneyValue(value: Maybe<Decimal | string>, fallback = EMPTY_VALUE): string {
  if (!hasValue(value)) return fallback
  const text = String(value).replaceAll(',', '').replace(/[¥$]/g, '').trim()
  const amount = Number(text)
  return Number.isFinite(amount) ? `¥${amount.toFixed(2)}` : fallback
}

export function moneyDisplay(
  display: Maybe<string>,
  raw: Maybe<Decimal>,
  fallback = EMPTY_VALUE,
): string {
  return hasValue(display) ? moneyValue(display, fallback) : moneyValue(raw, fallback)
}

export function durationValue(display: Maybe<string>, raw: Maybe<Decimal>): string {
  if (hasValue(display)) return String(display)
  if (!hasValue(raw)) return EMPTY_VALUE
  const milliseconds = Number(raw)
  if (!Number.isFinite(milliseconds)) return EMPTY_VALUE
  return milliseconds >= 1000 ? `${(milliseconds / 1000).toFixed(2)}s` : `${Math.round(milliseconds)}ms`
}

export function asChartNumber(value: Maybe<Decimal>): number | null {
  if (!hasValue(value)) return null
  const parsed = Number(value)
  return Number.isFinite(parsed) ? parsed : null
}

export function lastUpdated(value: Maybe<string>): string {
  if (!value) return EMPTY_VALUE
  const date = new Date(value)
  if (Number.isNaN(date.getTime())) return EMPTY_VALUE
  return new Intl.DateTimeFormat('zh-CN', {
    hour: '2-digit',
    minute: '2-digit',
    month: 'numeric',
    day: 'numeric',
  }).format(date)
}
