// 引擎 wire → 内联 SVG 图标（lobehub icons-static-svg，?raw）。
// 单一来源：AgentManageView / NewSessionDialog / MessageComposer 共用，避免各写一份映射。
// 优先彩色变体（*-color.svg，自带品牌 fill）；无彩色变体的引擎用 mono 版
// （fill=currentColor），调用方需给容器显式文字色，否则会继承页面色渲染成黑块。
import opencodeIcon from '@lobehub/icons-static-svg/icons/opencode.svg?raw'
import claudecodeIcon from '@lobehub/icons-static-svg/icons/claudecode-color.svg?raw'
import codexIcon from '@lobehub/icons-static-svg/icons/codex-color.svg?raw'
import geminiCliIcon from '@lobehub/icons-static-svg/icons/geminicli-color.svg?raw'
import clineIcon from '@lobehub/icons-static-svg/icons/cline.svg?raw'
import cursorIcon from '@lobehub/icons-static-svg/icons/cursor.svg?raw'
import hermesIcon from '@lobehub/icons-static-svg/icons/hermesagent.svg?raw'
import codebuddyIcon from '@lobehub/icons-static-svg/icons/codebuddy-color.svg?raw'
import kimiIcon from '@lobehub/icons-static-svg/icons/kimi-color.svg?raw'
import piIcon from '@lobehub/icons-static-svg/icons/pi.svg?raw'
import grokIcon from '@lobehub/icons-static-svg/icons/grok.svg?raw'

/** wire → 图标 SVG 文本（未知引擎空串占位）。 */
export const ENGINE_ICONS: Record<string, string> = {
  open_code: opencodeIcon,
  claude_code: claudecodeIcon,
  codex: codexIcon,
  gemini: geminiCliIcon,
  cline: clineIcon,
  cursor: cursorIcon,
  hermes: hermesIcon,
  code_buddy: codebuddyIcon,
  kimi_code: kimiIcon,
  pi: piIcon,
  grok: grokIcon,
}

/** 取引擎图标 SVG；未命中返回空串（调用方可回退通用图标）。 */
export function engineIcon(wire: string | null | undefined): string {
  return wire ? (ENGINE_ICONS[wire] ?? '') : ''
}
