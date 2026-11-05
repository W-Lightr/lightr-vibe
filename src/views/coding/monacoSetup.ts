// Monaco Editor worker 配置（Vite ?worker 语法）。
// 由 FileEditorView 在导入 monaco-editor-vue 之前先导入本模块，保证 Monaco 求值前
// worker 工厂已就位。后续任何使用 Monaco 的入口都必须先导入本模块。
//
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
