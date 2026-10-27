<script setup lang="ts">
// 计划生成器（对齐设计图 3）：频率下拉 + 时:分 + 预览。
//
// 只负责生成 cron 字符串；求值一律交给 Rust（预览与真实触发同源）。
// 「每 N 分钟 / 每小时」这类没有固定时分的形式走「自定义」，避免下拉里
// 出现一个时间输入却没意义的尴尬。
import { computed, ref, watch } from 'vue'
import { X } from 'lucide-vue-next'
import { formatDateTime } from '../../../utils/time'
import { scheduleLabel } from '../scheduleLabel'
import type { NextRunPreview } from '../../../types/automation'

const props = defineProps<{
  open: boolean
  cron: string
  timezone: string
  preview: NextRunPreview | null
}>()

const emit = defineEmits<{
  close: []
  confirm: [cron: string]
}>()

/** 频率选项：只保留带固定时分的（时间输入才有意义） */
const FREQUENCIES = [
  { value: 'daily', label: '每天' },
  { value: 'weekday', label: '工作日' },
  { value: 'week', label: '每周' },
  { value: 'month', label: '每月' },
  { value: 'custom', label: '自定义' },
] as const

type Frequency = (typeof FREQUENCIES)[number]['value']

const frequency = ref<Frequency>('weekday')
/** 「每周」用的星期（0=周日 … 6=周六） */
const weekday = ref(1)
/** 「每月」用的日 */
const monthDay = ref(1)
const hour = ref(9)
const minute = ref(0)
const custom = ref('')
const initialCron = ref('')

const WEEKDAY_OPTIONS = [
  { value: 1, label: '周一' },
  { value: 2, label: '周二' },
  { value: 3, label: '周三' },
  { value: 4, label: '周四' },
  { value: 5, label: '周五' },
  { value: 6, label: '周六' },
  { value: 0, label: '周日' },
]

/** 从已有表达式反推表单（尽力而为；推不出来就落「自定义」） */
function parseInitial(cron: string) {
  initialCron.value = cron
  custom.value = cron
  const fields = cron.trim().split(/\s+/)
  if (fields.length !== 5) {
    frequency.value = 'custom'
    return
  }
  const [mi, ho, dom, mon, dow] = fields
  const isNum = (v: string) => /^\d+$/.test(v)
  if (!isNum(mi) || !isNum(ho) || mon !== '*') {
    frequency.value = 'custom'
    return
  }
  hour.value = Number(ho)
  minute.value = Number(mi)
  if (dow === '1-5' && dom === '*') {
    frequency.value = 'weekday'
  } else if (dow === '*' && dom === '*') {
    frequency.value = 'daily'
  } else if (isNum(dow) && dom === '*') {
    frequency.value = 'week'
    weekday.value = Number(dow) === 7 ? 0 : Number(dow)
  } else if (dow === '*' && isNum(dom)) {
    frequency.value = 'month'
    monthDay.value = Number(dom)
  } else {
    frequency.value = 'custom'
  }
}

watch(
  () => props.open,
  (open) => {
    if (open) parseInitial(props.cron || '0 9 * * 1-5')
  },
)

/** 由表单生成表达式 */
const generated = computed(() => {
  if (frequency.value === 'custom') return custom.value.trim()
  const mi = String(minute.value)
  const ho = String(hour.value)
  switch (frequency.value) {
    case 'daily':
      return `${mi} ${ho} * * *`
    case 'weekday':
      return `${mi} ${ho} * * 1-5`
    case 'week':
      return `${mi} ${ho} * * ${weekday.value}`
    case 'month':
      return `${mi} ${ho} ${monthDay.value} * *`
    default:
      return custom.value.trim()
  }
})

const description = computed(() =>
  generated.value ? scheduleLabel(generated.value, '').replace(/ · .*$/, '') : '未填写',
)

const nextRunText = computed(() => {
  const at = props.preview?.nextRunAt
  return at ? formatDateTime(at) : null
})

/** 自定义输入变化时让父级重新求值（父级按 cron 值节流） */
function onCustomInput() {
  // 只更新本地；真正的预览由父级在 confirm 后触发，或在输入时由父级 watch cron 触发
  // 这里不 emit，避免每敲一个字就弹一次保存
}

function apply() {
  const value = generated.value
  if (!value) return
  emit('confirm', value)
}
</script>

<template>
  <div
    v-if="props.open"
    data-no-drag
    class="absolute inset-0 z-[60] flex items-center justify-center bg-neutral-900/25"
    @mousedown.self="emit('close')"
  >
    <div class="w-[460px] rounded-2xl bg-white px-6 pt-5 pb-5 shadow-xl">
      <!-- 标题 -->
      <div class="flex items-center justify-between">
        <span class="text-[15px] font-medium text-neutral-900">计划生成器</span>
        <button
          class="flex h-6 w-6 cursor-pointer items-center justify-center rounded text-neutral-400 hover:bg-neutral-100 hover:text-neutral-700"
          title="关闭"
          @click="emit('close')"
        >
          <X class="h-3.5 w-3.5" />
        </button>
      </div>

      <!-- 频率 -->
      <p class="mt-4 text-[11px] text-neutral-500">频率</p>
      <div class="mt-1.5 flex items-center gap-1.5">
        <div class="relative">
          <select
            v-model="frequency"
            class="h-9 cursor-pointer appearance-none rounded-full border border-neutral-200 bg-neutral-50 pr-7 pl-3.5 text-[12px] text-neutral-700 outline-none focus:border-neutral-400"
          >
            <option v-for="f in FREQUENCIES" :key="f.value" :value="f.value">
              {{ f.label }}
            </option>
          </select>
          <span
            class="pointer-events-none absolute top-1/2 right-2.5 -translate-y-1/2 text-[9px] text-neutral-400"
          >
            ▾
          </span>
        </div>

        <!-- 每周 / 每月 的附加选择 -->
        <div v-if="frequency === 'week'" class="relative">
          <select
            v-model="weekday"
            class="h-9 cursor-pointer appearance-none rounded-full border border-neutral-200 bg-neutral-50 pr-7 pl-3.5 text-[12px] text-neutral-700 outline-none focus:border-neutral-400"
          >
            <option v-for="d in WEEKDAY_OPTIONS" :key="d.value" :value="d.value">
              {{ d.label }}
            </option>
          </select>
          <span
            class="pointer-events-none absolute top-1/2 right-2.5 -translate-y-1/2 text-[9px] text-neutral-400"
          >
            ▾
          </span>
        </div>
        <div v-if="frequency === 'month'" class="relative">
          <select
            v-model="monthDay"
            class="h-9 cursor-pointer appearance-none rounded-full border border-neutral-200 bg-neutral-50 pr-7 pl-3.5 text-[12px] text-neutral-700 outline-none focus:border-neutral-400"
          >
            <option v-for="d in 28" :key="d" :value="d">{{ d }} 日</option>
          </select>
          <span
            class="pointer-events-none absolute top-1/2 right-2.5 -translate-y-1/2 text-[9px] text-neutral-400"
          >
            ▾
          </span>
        </div>
      </div>

      <!-- 时间 -->
      <template v-if="frequency !== 'custom'">
        <p class="mt-4 text-[11px] text-neutral-500">时间</p>
        <div class="mt-1.5 flex items-center gap-1.5">
          <input
            v-model.number="hour"
            type="number"
            min="0"
            max="23"
            class="h-9 w-16 rounded-lg border border-neutral-200 bg-neutral-50 text-center text-[13px] tabular-nums text-neutral-800 outline-none focus:border-neutral-400"
          />
          <span class="text-[13px] text-neutral-400">:</span>
          <input
            v-model.number="minute"
            type="number"
            min="0"
            max="59"
            class="h-9 w-16 rounded-lg border border-neutral-200 bg-neutral-50 text-center text-[13px] tabular-nums text-neutral-800 outline-none focus:border-neutral-400"
          />
        </div>
      </template>

      <!-- 自定义表达式 -->
      <template v-else>
        <p class="mt-4 text-[11px] text-neutral-500">cron 表达式（5 字段）</p>
        <input
          v-model="custom"
          class="mt-1.5 h-9 w-full rounded-lg border border-neutral-200 bg-neutral-50 px-3 font-mono text-[12px] text-neutral-800 outline-none focus:border-neutral-400"
          placeholder="*/5 * * * *"
          spellcheck="false"
          @input="onCustomInput"
        />
      </template>

      <!-- 预览卡片 -->
      <div class="mt-4 rounded-xl border border-neutral-200 px-3.5 py-3">
        <p class="text-[10px] text-neutral-400">预览</p>
        <p class="mt-1 flex flex-wrap items-baseline gap-x-2 text-[12px]">
          <span class="text-neutral-800">{{ description }}</span>
          <span class="font-mono text-[11px] text-neutral-400">{{ generated }}</span>
        </p>
        <p class="mt-1 text-[11px] text-neutral-500">
          <template v-if="nextRunText">下次运行：{{ nextRunText }}</template>
          <template v-else>该表达式不会再触发</template>
        </p>
      </div>

      <!-- 操作 -->
      <div class="mt-5 flex items-center justify-end gap-2">
        <button
          class="h-9 cursor-pointer px-3 text-[12px] text-neutral-500 hover:text-neutral-800"
          @click="emit('close')"
        >
          取消
        </button>
        <button
          class="h-9 cursor-pointer rounded-full bg-neutral-900 px-5 text-[12px] text-white hover:bg-neutral-800 disabled:cursor-not-allowed disabled:opacity-40"
          :disabled="!generated"
          @click="apply"
        >
          应用
        </button>
      </div>
    </div>
  </div>
</template>
