import { createApp } from 'vue'
import { createPinia } from 'pinia'
import App from './App.vue'
import router from './router'

import './assets/main.css'
import '@xterm/xterm/css/xterm.css'
import { initAppData } from './utils/appData'
import { useProjectStore } from './stores/project'
import { useSessionStore } from './stores/session'
import { initAcpListeners } from './services/acpService'
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

// ---- Monaco Editor worker 配置（Vite ?worker 语法）----
// 注意：monaco-editor 0.57+ 的 exports 映射为 "./*.js" -> "./esm/vs/*.js"，
// 因此这里使用短路径 + .js 后缀，而非 esm/vs 全路径。
import editorWorker from 'monaco-editor/editor/editor.worker.js?worker'
import jsonWorker from 'monaco-editor/language/json/json.worker.js?worker'
import cssWorker from 'monaco-editor/language/css/css.worker.js?worker'
import htmlWorker from 'monaco-editor/language/html/html.worker.js?worker'
import tsWorker from 'monaco-editor/language/typescript/ts.worker.js?worker'

;(self as unknown as { MonacoEnvironment: unknown }).MonacoEnvironment = {
  getWorker(_workerId: string, label: string) {
    switch (label) {
      case 'json':
        return new jsonWorker()
      case 'css':
      case 'scss':
      case 'less':
        return new cssWorker()
      case 'html':
      case 'handlebars':
      case 'razor':
        return new htmlWorker()
      case 'typescript':
      case 'javascript':
        return new tsWorker()
      default:
        return new editorWorker()
    }
  },
}

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
  void initAcpListeners()
  // 任务看板 ↔ ACP 会话联动：turn 结束回流订阅为模块级单订阅，必须与页面无关地
  // 在启动时装上（用户从任务页启动后切到编码页，turn 会在编码页结束）
  wireTaskSessionBridge()
  app.mount('#app')
}

void bootstrap()
