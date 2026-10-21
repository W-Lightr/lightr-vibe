// 看板拖拽（手写 Pointer Events，design §5）：不使用 HTML5 draggable / 拖拽库。
// 关键点：
//  1. 移动超过 4px 才进入拖拽，保证卡片点击（打开详情）不被吞掉；
//  2. 拖拽预览 Teleport 到 body，fixed 跟随指针（看板列是 overflow 滚动容器，会裁掉卡片）；
//  3. 落列判定用指针坐标与列矩形求交（不受子元素 pointer-events 影响）；
//  4. 同列实时重排只改「响应式顺序」，由调用方渲染——不手改 DOM，避免与 Vue 的
//     v-for 补丁算法打架（否则松手回弹/下一次渲染会把脏顺序带进列表）；
//  5. 结束后 300ms 内抑制 click，避免误开详情。
// 状态映射复用 boardColumns（allowedDrop / columnForStatus），不在此重复一份。
// 所有窗口监听在 onBeforeUnmount 清理；blur / 切后台 / Esc 时中止拖拽。

import { computed, onBeforeUnmount, ref } from 'vue'
import type { ComputedRef } from 'vue'
import type { TaskColumnId, TaskStatus } from '../../../types/task'
import { allowedDrop, columnForStatus } from '../boardColumns'

/** 进入拖拽的最小位移（px） */
const DRAG_THRESHOLD = 4
/** 松手后抑制 click 的时长（ms） */
const CLICK_SUPPRESS_MS = 300
/** 非法落列抖动的时长（ms） */
const SHAKE_MS = 420

export interface TaskDragOptions {
  /** 当前视图的项目筛选（null = 全部项目 → 禁止拖拽，design §5） */
  projectId: ComputedRef<string | null>
  /**
   * 发起跨列状态转移：只在 allowedDrop 返回非 null 时调用。
   * 由调用方接 taskStore.transition，本 composable 不碰 store。
   */
  onMoveToStatus: (taskId: string, to: TaskStatus, from: TaskStatus) => void
  /**
   * 同列松手：按新的列内顺序落库。
   * orderedIds 只含**可见**卡片（已隐藏的已归档/已取消项不参与，调用方负责合并）。
   */
  onReorder: (orderedIds: string[]) => void
}

export interface DragState {
  taskId: string
  /** 被拖任务当前状态（drop 时的 CAS 预期值） */
  status: TaskStatus
  /** 抓取点相对卡片左上角的偏移（让预览 1:1 跟手） */
  offsetX: number
  offsetY: number
  /** 卡片宽度（预览副本保持同宽） */
  width: number
  /** 指针 client 坐标 */
  clientX: number
  clientY: number
  /** 是否已越过阈值、真正在拖拽 */
  active: boolean
}

export function useTaskDrag(options: TaskDragOptions) {
  /** 当前拖拽态（null = 未拖拽） */
  const drag = ref<DragState | null>(null)
  /** 指针所在的合法目标列（仅合法时高亮，非法列不亮） */
  const hoverColumn = ref<TaskColumnId | null>(null)
  /** 正在抖动的任务 id（非法落列回弹时闪一次） */
  const shakingTaskId = ref<string | null>(null)

  /** 列 DOM 注册表：columnId → 列的滚动容器（只读，用于矩形命中测试） */
  const columnEls = new Map<TaskColumnId, HTMLElement>()
  /** 卡片 DOM 注册表：taskId → 卡片根元素（只读，用于抓取点偏移与实时重排） */
  const cardEls = new Map<string, HTMLElement>()

  /** 同列拖拽期间的本地顺序（null = 未在重排）；由调用方拿去渲染，不手改 DOM */
  const localOrder = ref<string[] | null>(null)

  let pending: {
    taskId: string
    status: TaskStatus
    startX: number
    startY: number
    offsetX: number
    offsetY: number
    width: number
  } | null = null

  let suppressClickUntil = 0
  let shakeTimer: number | null = null

  /** 拖拽始终可用：跨列转移与项目无关；同列排序只在单项目视图提交
   *  （sortOrder 是项目内语义，混合视图没有可持久化的顺序）。 */
  const dragEnabled = computed(() => true)

  /** 同列排序是否可提交（全部项目视图下拖拽只做跨列转移） */
  const reorderEnabled = computed(() => options.projectId.value !== null)

  /** 卡片根元素注册 / 注销（TaskCard 用 ref 回调写入） */
  function registerCard(taskId: string, el: HTMLElement | null): void {
    if (el) cardEls.set(taskId, el)
    else cardEls.delete(taskId)
  }

  /** 列滚动容器注册 / 注销（TaskColumn 用 ref 回调写入） */
  function registerColumn(columnId: TaskColumnId, el: HTMLElement | null): void {
    if (el) columnEls.set(columnId, el)
    else columnEls.delete(columnId)
  }

  function onPointerMove(e: PointerEvent): void {
    const state = drag.value
    if (!state) return
    if (!state.active) {
      const moved =
        Math.abs(e.clientX - (pending?.startX ?? e.clientX)) +
        Math.abs(e.clientY - (pending?.startY ?? e.clientY))
      if (!pending || moved <= DRAG_THRESHOLD) return
    }
    drag.value = { ...state, active: true, clientX: e.clientX, clientY: e.clientY }
    hoverColumn.value = legalColumnAt(e.clientX, e.clientY)
    reorderPreview(state.taskId, state.status, e.clientX, e.clientY)
  }

  function onPointerUp(e: PointerEvent): void {
    const state = drag.value
    const candidate = pending
    const wasActive = state?.active === true
    const endX = e.clientX
    const endY = e.clientY
    // 松手时先把「本地重排结果」取出来（cleanup 会清空 drag 态）
    const previewOrder = localOrder.value
    cleanup()
    if (!wasActive || !candidate) return
    // 抑制紧随其后的 click（否则会打开详情）
    suppressClickUntil = Date.now() + CLICK_SUPPRESS_MS

    const columnId = hitColumn(endX, endY)
    if (!columnId) return
    const currentColumn = columnForStatus(candidate.status)
    if (columnId === currentColumn) {
      // 全部项目视图：顺序没有跨项目语义，同列拖动视为取消（回弹，不发请求）
      if (!reorderEnabled.value) return
      const ordered = previewOrder ?? readColumnOrder(candidate.taskId)
      if (ordered && ordered.length > 0) options.onReorder(ordered)
      return
    }
    const to = allowedDrop(candidate.status, columnId)
    if (!to) {
      // 非法落列：不发请求，回弹 + 抖动一次
      flashShake(candidate.taskId)
      return
    }
    options.onMoveToStatus(candidate.taskId, to, candidate.status)
  }

  /** 中止拖拽（Esc / 失焦 / 切后台）：不发请求，直接回弹 */
  function abortDrag(): void {
    const wasActive = drag.value?.active === true
    cleanup()
    if (wasActive) suppressClickUntil = Date.now() + CLICK_SUPPRESS_MS
  }

  /** 清理一次拖拽会话：移除全部窗口监听 + 复位状态。
   * localOrder 归 null，调用方据此回到 store 顺序（回弹）。
   * 导出给页面用于「视图切换」（路由参数变化）时强制复位，避免上一次拖拽的
   * 本地顺序 / 悬停列 / 窗口监听残留到新看板。 */
  function cleanup(): void {
    window.removeEventListener('pointermove', onPointerMove)
    window.removeEventListener('pointerup', onPointerUp)
    window.removeEventListener('pointercancel', onPointerUp)
    window.removeEventListener('blur', abortDrag)
    window.removeEventListener('keydown', onKeyDown)
    document.removeEventListener('visibilitychange', onVisibilityChange)
    pending = null
    drag.value = null
    hoverColumn.value = null
    localOrder.value = null
  }

  function onKeyDown(e: KeyboardEvent): void {
    if (e.key === 'Escape') abortDrag()
  }

  function onVisibilityChange(): void {
    if (document.visibilityState === 'hidden') abortDrag()
  }

  /**
   * 卡片 pointerdown：记录候选拖拽，等位移越过阈值才升级。
   * 全部项目视图直接忽略（cursor-default，不挂监听）。
   */
  function onCardPointerDown(e: PointerEvent, taskId: string, status: TaskStatus): void {
    if (e.button !== 0 || !dragEnabled.value || drag.value) return
    const el = cardEls.get(taskId)
    if (!el) return
    const rect = el.getBoundingClientRect()
    pending = {
      taskId,
      status,
      startX: e.clientX,
      startY: e.clientY,
      offsetX: e.clientX - rect.left,
      offsetY: e.clientY - rect.top,
      width: rect.width,
    }
    drag.value = {
      taskId,
      status,
      offsetX: pending.offsetX,
      offsetY: pending.offsetY,
      width: rect.width,
      clientX: e.clientX,
      clientY: e.clientY,
      active: false,
    }
    window.addEventListener('pointermove', onPointerMove)
    window.addEventListener('pointerup', onPointerUp)
    window.addEventListener('pointercancel', onPointerUp)
    window.addEventListener('blur', abortDrag)
    window.addEventListener('keydown', onKeyDown)
    document.addEventListener('visibilitychange', onVisibilityChange)
  }

  /** 指针落在哪一列（先命中先返回；列在 flex 布局中互不重叠） */
  function hitColumn(x: number, y: number): TaskColumnId | null {
    for (const [columnId, el] of columnEls) {
      const rect = el.getBoundingClientRect()
      if (x >= rect.left && x <= rect.right && y >= rect.top && y <= rect.bottom) return columnId
    }
    return null
  }

  /** 指针所在的合法目标列：同列恒合法；跨列仅 allowedDrop 允许时返回 */
  function legalColumnAt(x: number, y: number): TaskColumnId | null {
    const state = drag.value
    if (!state) return null
    const columnId = hitColumn(x, y)
    if (!columnId) return null
    if (columnId === columnForStatus(state.status)) return columnId
    return allowedDrop(state.status, columnId) ? columnId : null
  }

  /**
   * 同列实时重排：用指针 y 与被拖卡片外的各卡片中线比较，算出新顺序写进 localOrder。
   * 只算「顺序」，不动 DOM——顺序由调用方交给 v-for 渲染，Vue 负责移动节点。
   */
  function reorderPreview(taskId: string, status: TaskStatus, x: number, y: number): void {
    // 全部项目视图不排序：同列拖动不做实时重排（松手直接回弹）
    if (!reorderEnabled.value) return
    const columnId = hitColumn(x, y)
    if (!columnId || columnId !== columnForStatus(status)) return
    const order = localOrder.value ?? readColumnOrder(taskId)
    if (!order || order.length === 0) return
    if (!order.includes(taskId)) return
    // 目标插入位：指针 y 与其它卡片中线比较，取第一个「指针在它上半区」的位置
    const rest = order.filter((id) => id !== taskId)
    let insertAt = rest.length
    for (let i = 0; i < rest.length; i += 1) {
      const node = cardEls.get(rest[i])
      if (!node) continue
      const rect = node.getBoundingClientRect()
      if (y < rect.top + rect.height / 2) {
        insertAt = i
        break
      }
    }
    const next = [...rest.slice(0, insertAt), taskId, ...rest.slice(insertAt)]
    // 值相同则不触发重渲染（pointermove 很频繁）
    if (order.length === next.length && order.every((id, i) => id === next[i])) return
    localOrder.value = next
  }

  /** 读被拖卡片所在列当前的 DOM 顺序 → 任务 id 列表（仅注册过的卡片，即当前可见卡片） */
  function readColumnOrder(taskId: string): string[] | null {
    const el = cardEls.get(taskId)
    const parent = el?.parentElement
    if (!parent) return null
    const byNode = new Map<Element, string>()
    for (const [id, node] of cardEls) byNode.set(node, id)
    const ordered: string[] = []
    for (const node of [...parent.children]) {
      const id = byNode.get(node)
      if (id) ordered.push(id)
    }
    return ordered.length > 0 ? ordered : null
  }

  /** 非法落列：抖动一次（CSS animate-shake） */
  function flashShake(id: string): void {
    shakingTaskId.value = id
    if (shakeTimer !== null) window.clearTimeout(shakeTimer)
    shakeTimer = window.setTimeout(() => {
      shakingTaskId.value = null
      shakeTimer = null
    }, SHAKE_MS)
  }

  /** 是否抑制本次 click（拖拽刚结束） */
  function shouldSuppressClick(): boolean {
    return Date.now() < suppressClickUntil
  }

  onBeforeUnmount(() => {
    cleanup()
    if (shakeTimer !== null) window.clearTimeout(shakeTimer)
    shakeTimer = null
    columnEls.clear()
    cardEls.clear()
  })

  return {
    drag,
    hoverColumn,
    shakingTaskId,
    /** 同列拖拽中的本地顺序（null = 用 store 顺序） */
    localOrder,
    dragEnabled,
    /** 同列排序是否可用（全部项目视图为 false：可跨列拖，不排序） */
    reorderEnabled,
    registerCard,
    registerColumn,
    onCardPointerDown,
    shouldSuppressClick,
    /** 中止并复位当前拖拽（视图切换时由页面调用） */
    resetDrag: abortDrag,
  }
}
