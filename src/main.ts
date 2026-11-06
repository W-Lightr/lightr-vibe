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
// 订阅 ACP 事件 → mount。列表需在任何视图渲染前就绪（侧栏分组直接消费）。
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
  try {
    await delegation.load()
  } catch {
    // 委派数据不可读不阻塞普通会话；默认关闭仍保持安全。
  }
  onDelegationStarted(({ record }) => {
    delegation.upsert(record)
    const sessionStore = useSessionStore(pinia)
    const child = sessionStore.registerDelegatedSession(record)
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
    delegation.upsert(record)
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
  void initAcpListeners()
  // 任务看板 ↔ ACP 会话联动：turn 结束回流订阅为模块级单订阅，必须与页面无关地
  // 在启动时装上（用户从任务页启动后切到编码页，turn 会在编码页结束）
  wireTaskSessionBridge()
  app.mount('#app')
}

void bootstrap()
