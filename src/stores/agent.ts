// Agent 管理 store：检测状态、预检项、OpenCode 配置缓存、环境变量与写锁。
// 唯一数据入口是 opencodeService（Tauri/mock 双通道），组件不直接调 service。
// 写锁 writing：防止并发保存互相覆盖（PRD R7 并发保护）。

import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import type {
  AgentStatus,
  DetectResult,
  EnvVar,
  OpencodeConfigView,
  PermissionAction,
  PrecheckItem,
} from '../types/agent'
import { PERMISSION_KEYS } from '../types/agent'
import { normalizeConfig, opencodeService } from '../services/opencodeService'
import { patchConfig, type ConfigEdit } from '../utils/jsonc'
import { isTauri } from '../utils/isTauri'

/** 保存失败的可判别错误：conflict 需要前端引导重新加载 */
export type SaveError = { kind: 'conflict' } | { kind: 'error'; message: string }

export const useAgentStore = defineStore('agent', () => {
  // ---- 检测状态 ----
  const detecting = ref(false)
  const detectResult = ref<DetectResult | null>(null)
  /** 浏览器 dev 环境标记：界面显示 mock 提示条（PRD R3） */
  const browserMode = ref(!isTauri())

  // ---- 配置缓存 ----
  /** opencode.jsonc 原文（补丁基于它生成） */
  const configText = ref<string | null>(null)
  /** 读取时的 mtime，写回时传给 Rust 做冲突校验 */
  const configMtime = ref<number | null>(null)
  const configPath = ref('')
  const configLoaded = ref(false)

  // ---- 环境变量 ----
  const envVars = ref<EnvVar[]>([])
  const envLoaded = ref(false)

  // ---- UI 状态 ----
  /** OpenCode 启用开关：本期仅记录不生效（design D5） */
  const enabled = ref(true)
  /** 写锁：保存期间拒绝重入 */
  const writing = ref(false)
  const configError = ref<string | null>(null)

  const configView = computed<OpencodeConfigView | null>(() => {
    if (!configText.value) return null
    try {
      return normalizeConfig(configText.value)
    } catch {
      // 解析失败时 configError 已在 loadConfig 中记录，这里返回 null 让 UI 走引导分支
      return null
    }
  })

  /** 预检四项：版本状态 / Platform / 配置文件 / ACP 能力（design §2.1） */
  const prechecks = computed<PrecheckItem[]>(() => {
    const d = detectResult.value
    if (!d) return []
    return [
      {
        id: 'version',
        label: '版本状态',
        status: d.version ? 'pass' : 'fail',
        detail: d.version ?? (d.detail ?? '未解析到版本号'),
      },
      {
        id: 'platform',
        label: 'Platform',
        status: d.binPath ? 'pass' : 'fail',
        detail: d.binPath ? `${d.platform} · ${d.binPath}` : `${d.platform} · PATH 中未找到 opencode`,
      },
      {
        id: 'config',
        label: '配置文件',
        status: d.configExists ? 'pass' : 'fail',
        detail: d.configExists ? d.configPath : `${d.configPath}（不存在）`,
      },
      {
        id: 'acp',
        label: 'ACP 能力',
        status: d.acpOk ? 'pass' : 'fail',
        detail: d.acpOk ? 'opencode acp 子命令可用（仅探测，未启动服务）' : 'acp --help 探测失败',
      },
    ]
  })

  /** 列表徽章状态：检测中 spin / PASS 绿 / FAIL 红 / 未安装灰 */
  const agentStatus = computed<AgentStatus>(() => {
    if (detecting.value) return 'detecting'
    const d = detectResult.value
    if (!d) return 'unknown'
    if (!d.installed) return 'not-installed'
    return d.version && d.acpOk ? 'pass' : 'fail'
  })

  /** 执行检测并顺带加载配置与环境变量（进入 Agent 管理页时调用）。
   * 重入保护：检测进行中直接返回（quality-guidelines 规则 7）。 */
  async function detect(): Promise<void> {
    if (detecting.value) return
    detecting.value = true
    try {
      const result = await opencodeService.detect()
      detectResult.value = result
      if (result.installed && result.configExists) {
        // 配置读取失败不应让整个检测崩溃：记录错误，UI 走引导分支
        try {
          await loadConfig()
        } catch (e) {
          configError.value = e instanceof Error ? e.message : '配置读取失败'
          configText.value = null
          configLoaded.value = true
        }
      } else {
        configText.value = null
        configMtime.value = null
        configLoaded.value = true
      }
      // 环境变量文件读取失败不阻断检测（应用级文件损坏/缺失时按空处理）
      try {
        await loadEnv()
      } catch {
        envVars.value = []
        envLoaded.value = true
      }
    } finally {
      detecting.value = false
    }
  }

  /** 读 opencode.jsonc 原文 + mtime 并缓存 */
  async function loadConfig(): Promise<void> {
    configError.value = null
    const result = await opencodeService.readConfig()
    configText.value = result.text
    configMtime.value = result.mtimeMs
    configPath.value = result.path
    configLoaded.value = true
    if (result.text !== null) {
      try {
        normalizeConfig(result.text)
      } catch (e) {
        configError.value = e instanceof Error ? e.message : '配置解析失败'
      }
    }
  }

  /** 读应用级环境变量文件并解析为行编辑数据 */
  async function loadEnv(): Promise<void> {
    const result = await opencodeService.readEnvFile()
    envVars.value = parseEnvText(result.text)
    envLoaded.value = true
  }

  /** 补丁写回的公共路径：生成新文本 → Rust 校验 mtime → 更新缓存 */
  async function writePatched(edits: ConfigEdit[]): Promise<SaveError | null> {
    if (writing.value) return { kind: 'error', message: '正在保存中，请稍候' }
    if (configText.value === null) return { kind: 'error', message: '配置未加载' }
    writing.value = true
    try {
      const newText = patchConfig(configText.value, edits)
      const result = await opencodeService.writeConfig(newText, configMtime.value)
      configText.value = newText
      configMtime.value = result.mtimeMs
      return null
    } catch (e) {
      if (e === 'conflict' || (e instanceof Error && e.message.includes('conflict'))) {
        return { kind: 'conflict' }
      }
      return { kind: 'error', message: e instanceof Error ? e.message : String(e) }
    } finally {
      writing.value = false
    }
  }

  /** 保存主模型 / 小模型（kind: model → 顶层 model；small → 顶层 small_model）。
   * value 为 null = 清空下拉 → 删除该键（jsonc-parser 对 undefined 执行删除），
   * 恢复 opencode 自身默认选择，而不是写入非法的 null 值。 */
  async function saveModel(
    kind: 'model' | 'small',
    value: string | null,
  ): Promise<SaveError | null> {
    // 官方 schema 键为 snake_case（research §3）
    const key = kind === 'model' ? 'model' : 'small_model'
    return writePatched([{ path: [key], value: value ?? undefined }])
  }

  /** 保存单个权限键；action 为 null = 恢复「默认（继承）」（从配置中删除该键）。
   * 若原配置是单一 action 字符串（schema 合法写法，归一化为 '*'），
   * 先展开为完整对象再写入用户选择，避免在非对象节点上做嵌套补丁。 */
  async function savePermission(
    key: string,
    action: PermissionAction | null,
  ): Promise<SaveError | null> {
    const wildcard = configView.value?.permissions['*']
    if (wildcard) {
      const expanded: Record<string, PermissionAction> = {}
      for (const { key: k } of PERMISSION_KEYS) expanded[k] = wildcard
      if (action) expanded[key] = action
      else delete expanded[key]
      return writePatched([{ path: ['permission'], value: expanded }])
    }
    return writePatched([{ path: ['permission', key], value: action ?? undefined }])
  }

  /** 保存单个 provider 的 apiKey */
  async function saveApiKey(providerKey: string, apiKey: string): Promise<SaveError | null> {
    return writePatched([{ path: ['provider', providerKey, 'options', 'apiKey'], value: apiKey }])
  }

  /** 保存环境变量到应用级文件（纯 JSON，无注释保真问题） */
  async function saveEnv(vars: EnvVar[]): Promise<SaveError | null> {
    if (writing.value) return { kind: 'error', message: '正在保存中，请稍候' }
    writing.value = true
    try {
      const record: Record<string, string> = {}
      for (const v of vars) {
        const key = v.key.trim()
        if (key) record[key] = v.value
      }
      const text = `${JSON.stringify({ vars: record }, null, 2)}\n`
      await opencodeService.writeEnvFile(text)
      envVars.value = vars.filter((v) => v.key.trim())
      return null
    } catch (e) {
      return { kind: 'error', message: e instanceof Error ? e.message : String(e) }
    } finally {
      writing.value = false
    }
  }

  /** 把 { vars: {K:V} } JSON 文本解析为行数据；损坏/缺失时返回空列表 */
  function parseEnvText(text: string | null): EnvVar[] {
    if (!text) return []
    try {
      const parsed: unknown = JSON.parse(text)
      if (
        typeof parsed === 'object' &&
        parsed !== null &&
        'vars' in parsed &&
        typeof (parsed as { vars: unknown }).vars === 'object'
      ) {
        const vars = (parsed as { vars: Record<string, unknown> }).vars
        return Object.entries(vars)
          .filter(([, v]) => typeof v === 'string')
          .map(([k, v]) => ({ key: k, value: v as string }))
      }
    } catch {
      // 文件损坏按空处理，保存时会覆盖为合法 JSON
    }
    return []
  }

  return {
    detecting,
    detectResult,
    browserMode,
    configText,
    configMtime,
    configPath,
    configLoaded,
    configView,
    configError,
    prechecks,
    agentStatus,
    envVars,
    envLoaded,
    enabled,
    writing,
    detect,
    loadConfig,
    loadEnv,
    saveModel,
    savePermission,
    saveApiKey,
    saveEnv,
  }
})
