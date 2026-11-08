import { createApp } from 'vue'
import { createPinia } from 'pinia'
import App from './App.vue'
import router from './router'

import './assets/main.css'
import { initAppData } from './utils/appData'
import { useProjectStore } from './stores/project'
import { useSessionStore } from './stores/session'
import { useWorkspaceStore } from './stores/workspace'
import { useDelegationStore } from './stores/delegation'
import { initAcpListeners } from './services/acpService'
import { onDelegationStarted, onDelegationUpdated } from './services/acpEvents'
import { handleExternalLinkClick } from './utils/externalLink'
import { wireTaskSessionBridge } from './views/tasks/taskSessionBridge'

// 全局唯一入口：任何 <a href> 点击都以外部方式打开（默认浏览器 / 新标签页），
// 阻止 Tauri WebView 就地导航把应用界面替换掉。仅此一处处理链接，
// 组件内不要再加局部点击处理（否则同一次点击开两个标签）。
document.addEventListener(
  'click',
  (e) => {
    handleExternalLinkClick(e)
  },
  true,
)

// Monaco worker 配置与 xterm 样式不在启动路径导入：
// - worker 配置见 src/views/coding/monacoSetup.ts（随编辑器 chunk 按需加载）
// - xterm CSS 由 TerminalPanel.vue 导入（随终端 chunk 按需加载）

const app = createApp(App)

const pinia = createPinia()
app.use(pinia)
app.use(router)

// 启动流程：解析数据目录 → 加载项目索引 → 加载会话索引（懒加载消息）→
// 完成 ACP/委派事件订阅 → 恢复委派投影 → mount。列表需在任何视图渲染前就绪。
async function bootstrap() {
  await initAppData()
  try {
    await useProjectStore(pinia).load()
  } catch {
    // 索引加载失败不阻塞启动：以空项目列表进入，用户可重新打开文件夹
  }
  try {
    await useSessionStore(pinia).load()
  } catch {
    // 会话索引损坏/缺失不阻塞启动：以空会话列表进入
  }
  const delegation = useDelegationStore(pinia)
  const sessionStore = useSessionStore(pinia)

  async function projectDelegation(record: Parameters<typeof delegation.upsert>[0], openChild: boolean): Promise<void> {
    delegation.upsert(record)
    const child = sessionStore.registerDelegatedSession(record)
    // 卡片由持久化 lifecycle record 驱动，而非 adapter 给出的可变工具标题。
    await sessionStore.ensureDelegationCard(record)
    if (openChild) {
      // 子代理启动即切到既有右侧查看器，让流式进度与待处理交互无需额外点击即可可见。
      // 委派卡片仍保留入口，供用户关闭抽屉后再次打开。
      useWorkspaceStore(pinia).openDelegatedChild(child.id)
    }
  }

  onDelegationStarted(({ record }) => {
    void projectDelegation(record, true)
    // projectDelegation() 在第一个 await 前同步注册子会话，供紧随其后的
    // ACP update 与初始 prompt 投影使用。
    const child = sessionStore.get(record.childSessionId)
    if (!child) return
    // 子 prompt 从 Rust broker 发出，没有经过 useConversation.send；这里补齐
    // 本地用户消息与生成态，使 ACP 的流式 update 能自然写入子会话 transcript。
    if (child.messages.length === 0) {
      sessionStore.pushMessage(record.childSessionId, {
        role: 'user', type: 'text', content: record.task, status: 'done',
      })
    }
    sessionStore.setStatus(record.childSessionId, 'generating')
    useWorkspaceStore(pinia).setGenerating(record.childSessionId, true)
  })
  onDelegationUpdated((record) => {
    void projectDelegation(record, false)
    if (!['completed', 'failed', 'canceled', 'interrupted'].includes(record.status)) return
    const sessionStore = useSessionStore(pinia)
    const finalStatus = record.status === 'completed' ? 'done' : 'stopped'
    for (const message of sessionStore.get(record.childSessionId)?.messages ?? []) {
      if (message.status === 'streaming') {
        sessionStore.patchMessage(record.childSessionId, message.id, { status: finalStatus })
      }
    }
    sessionStore.setStatus(record.childSessionId, record.status === 'failed' ? 'error' : 'idle')
    useWorkspaceStore(pinia).setGenerating(record.childSessionId, false)
    void sessionStore.persistMessages(record.childSessionId)
  })

  // 监听器必须完全注册后才 mount：否则子代理刚创建就可能丢失 started 事件，
  // 右侧查看器和父会话卡片都会缺席。
  await initAcpListeners()
  try {
    await delegation.load()
    // 重启恢复走同一投影路径，但不抢占用户当前正在看的会话。
    await Promise.all(delegation.records.map((record) => projectDelegation(record, false)))
  } catch {
    // 委派数据不可读不阻塞普通会话；默认关闭仍保持安全。
  }
  // 任务看板 ↔ ACP 会话联动：turn 结束回流订阅为模块级单订阅，必须与页面无关地
  // 在启动时装上（用户从任务页启动后切到编码页，turn 会在编码页结束）
  wireTaskSessionBridge()
  app.mount('#app')
}

void bootstrap().catch((error) => {
  console.error('[bootstrap] ACP lifecycle listeners were not ready; app was not mounted', error)
})
