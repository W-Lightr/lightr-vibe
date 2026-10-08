// JSONC 安全编辑封装：用 jsonc-parser 的 modify() 生成最小编辑补丁，
// 保留用户配置里的注释、键序与缩进（PRD R7 / design D2）。
// why 在前端做补丁：jsonc-parser 是唯一成熟的最小编辑实现，Rust 侧无等价 crate；
// Rust 只负责读原文与写最终文本。

import { applyEdits, modify, parse, type ModificationOptions } from 'jsonc-parser'

/** 单条编辑：path 为 JSON 路径（对象键或数组下标），value 为写入值（undefined = 删除键） */
export interface ConfigEdit {
  path: (string | number)[]
  value: unknown
}

/** 检测文本换行风格：默认 LF，出现 CRLF 则用 CRLF（保持用户原格式） */
function detectEol(text: string): '\n' | '\r\n' {
  return text.includes('\r\n') ? '\r\n' : '\n'
}

/**
 * 解析 JSONC 文本（允许注释/尾逗号）。解析失败抛出带错误的 Error。
 * 返回 unknown：调用方需自行收窄形状。
 */
export function parseJsonc(text: string): unknown {
  const errors: { error: number; offset: number; length: number }[] = []
  const value = parse(text, errors, { allowTrailingComma: true }) as unknown
  if (errors.length > 0) {
    throw new Error(`JSONC 解析失败（${errors.length} 处错误，首个偏移 ${errors[0].offset}）`)
  }
  return value
}

/**
 * 对 JSONC 原文批量应用最小编辑，返回补丁后的完整文本。
 * 多条编辑依次 modify→applyEdits，避免自行拼接字符串破坏格式。
 */
export function patchConfig(text: string, edits: ConfigEdit[]): string {
  let current = text
  const options: ModificationOptions = {
    formattingOptions: {
      eol: detectEol(text),
      insertSpaces: true,
      tabSize: 2,
    },
  }
  for (const edit of edits) {
    const mods = modify(current, edit.path, edit.value, options)
    current = applyEdits(current, mods)
  }
  return current
}
