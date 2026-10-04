// 消息列表滚动控制：
// - 每个会话独立记忆滚动位置（切换标签恢复）
// - 仅当用户处于底部附近时才自动跟随流式输出
// - 远离底部时显示"回到最新"浮动按钮

import { nextTick, ref, type Ref } from 'vue'

const BOTTOM_THRESHOLD = 80

// 模块级存储：会话切换导致组件重挂载时滚动位置不丢失
const positions = new Map<string, number>()

export function useConversationScroll(elRef: Ref<HTMLElement | null>) {
  const atBottom = ref(true)
  const showBackToLatest = ref(false)
  let currentKey = ''

  function isNearBottom(el: HTMLElement) {
    return el.scrollHeight - el.scrollTop - el.clientHeight <= BOTTOM_THRESHOLD
  }

  function updateFlags(el: HTMLElement) {
    atBottom.value = isNearBottom(el)
    showBackToLatest.value = !atBottom.value && el.scrollHeight - el.clientHeight > BOTTOM_THRESHOLD
  }

  function onScroll() {
    const el = elRef.value
    if (!el) return
    positions.set(currentKey, el.scrollTop)
    updateFlags(el)
  }

  /** 切换会话：恢复该会话的滚动位置（无记录则滚到底部） */
  async function restore(key: string) {
    currentKey = key
    await nextTick()
    const el = elRef.value
    if (!el) return
    const saved = positions.get(key)
    el.scrollTop = saved ?? el.scrollHeight
    updateFlags(el)
  }

  /** 消息变化时调用：处于底部则跟随滚动 */
  async function followIfAtBottom() {
    const el = elRef.value
    if (!el) return
    if (atBottom.value) {
      await nextTick()
      el.scrollTop = el.scrollHeight
    }
    updateFlags(el)
  }

  async function scrollToBottom() {
    const el = elRef.value
    if (!el) return
    await nextTick()
    el.scrollTo({ top: el.scrollHeight, behavior: 'smooth' })
    atBottom.value = true
    showBackToLatest.value = false
  }

  return { onScroll, restore, followIfAtBottom, scrollToBottom, showBackToLatest }
}
