// JSONC 往返测试（implement D18）：读真实 opencode.jsonc 副本 →
// 用与 src/utils/jsonc.ts 相同的 modify/applyEdits 逻辑改 model →
// diff 确认仅目标键变化、注释与其余格式保留。
const fs = require('fs')
const os = require('os')
const path = require('path')
const { parse, modify, applyEdits } = require('jsonc-parser')

const src = path.join(os.homedir(), '.config', 'opencode', 'opencode.jsonc')
const text = fs.readFileSync(src, 'utf8')

// 与 src/utils/jsonc.ts patchConfig 完全一致的参数
function detectEol(t) {
  return t.includes('\r\n') ? '\r\n' : '\n'
}
function patchConfig(t, edits) {
  let current = t
  const options = { formattingOptions: { eol: detectEol(t), insertSpaces: true, tabSize: 2 } }
  for (const edit of edits) {
    const mods = modify(current, edit.path, edit.value, options)
    current = applyEdits(current, mods)
  }
  return current
}

// 1) 改已存在的顶层 model
const newValue = 'magpie/commandcode/z-ai/glm-5.3-flash'
const patched = patchConfig(text, [{ path: ['model'], value: newValue }])

// 2) 逐行 diff
const a = text.split('\n')
const b = patched.split('\n')
const diffs = []
for (let i = 0; i < Math.max(a.length, b.length); i++) {
  if (a[i] !== b[i]) diffs.push({ line: i + 1, before: a[i], after: b[i] })
}
console.log('=== diff（应仅 "model" 一行） ===')
for (const d of diffs) {
  console.log(`L${d.line}\n- ${d.before}\n+ ${d.after}`)
}

// 3) 解析验证：仅 model 键值变化，其余全部保留
const before = parse(text, [], { allowTrailingComma: true })
const after = parse(patched, [], { allowTrailingComma: true })
const okModel = after.model === newValue
before.model = newValue
const okRest = JSON.stringify(before) === JSON.stringify(after)
console.log('model 已更新:', okModel)
console.log('其余键值不变:', okRest)

// 4) 注释保留验证：若原文含 // 或 /* 注释行，patched 必须仍含相同注释行
const commentLines = a.filter((l) => /^\s*(\/\/|\/\*|\*)/.test(l))
const commentKept = commentLines.every((l) => patched.includes(l))
console.log('注释行数:', commentLines.length, '注释保留:', commentKept)

// 5) 新增键测试：删除一个不存在的键 → 应为 no-op；permission 新键写入
const noop = patchConfig(text, [{ path: ['permission', 'bash'], value: 'ask' }])
const noopParsed = parse(noop, [], { allowTrailingComma: true })
console.log('新增 permission.bash 写入成功:', noopParsed.permission && noopParsed.permission.bash === 'ask')
const noopDiffCount = text.split('\n').filter((l, i) => l !== noop.split('\n')[i]).length
console.log('permission 补丁影响行数（新增块，应 >0 且原内容保留）:', noopDiffCount)

if (okModel && okRest && commentKept) {
  console.log('ROUNDTRIP PASS')
} else {
  console.log('ROUNDTRIP FAIL')
  process.exitCode = 1
}
