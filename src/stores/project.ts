// 项目（真实文件夹工作区）仓库：项目索引持久化于
// ~/.lightr-vibe/lightr-vibe/projects.json（父任务命名空间约束，禁止触碰根下
// 其他应用既有文件）。目录树与文件内容不在 store 内——懒加载经 fsService。
// removeProject 仅删索引不删磁盘文件（design D4：其会话 projectId 置空归入聊天组）。

import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import type { Project } from '../types/project'
import { fsService, projectIdFromPath } from '../services/fsService'
import { mockSeedProjects } from '../services/projectMock'
import { projectsIndexFileSync } from '../utils/appData'
import { isTauri } from '../utils/isTauri'
import { createId } from '../utils/id'

export const useProjectStore = defineStore('project', () => {
  const projects = ref<Project[]>([])
  const loaded = ref(false)

  const byId = computed(() => {
    const map = new Map<string, Project>()
    for (const p of projects.value) map.set(p.id, p)
    return map
  })

  /** 「最近」分组：lastOpenedAt 倒序前 5（PRD R3） */
  const recentProjects = computed(() =>
    [...projects.value].sort((a, b) => b.lastOpenedAt - a.lastOpenedAt).slice(0, 5),
  )

  function get(projectId: string | null | undefined): Project | null {
    return projectId ? (byId.value.get(projectId) ?? null) : null
  }

  /** 按根路径查找已有项目（重复打开同一路径时复用，不产生重复行） */
  function findByRootPath(rootPath: string): Project | null {
    const key = rootPath.replace(/\\/g, '/').replace(/\/+$/, '').toLowerCase()
    return (
      projects.value.find(
        (p) => p.rootPath.replace(/\\/g, '/').replace(/\/+$/, '').toLowerCase() === key,
      ) ?? null
    )
  }

  function parseProjects(text: string): Project[] {
    try {
      const raw: unknown = JSON.parse(text)
      if (!Array.isArray(raw)) return []
      return raw
        .filter(
          (p): p is Project =>
            typeof p === 'object' &&
            p !== null &&
            typeof (p as Project).id === 'string' &&
            typeof (p as Project).name === 'string' &&
            typeof (p as Project).rootPath === 'string' &&
            typeof (p as Project).addedAt === 'number' &&
            typeof (p as Project).lastOpenedAt === 'number',
        )
    } catch {
      return []
    }
  }

  /** 启动加载：读 projects.json；浏览器 mock 通道使用内存种子项目 */
  async function load(): Promise<void> {
    const path = projectsIndexFileSync()
    const text = await fsService.readText(path)
    if (text !== null) {
      projects.value = parseProjects(text)
    } else if (!isTauri()) {
      projects.value = mockSeedProjects()
    } else {
      projects.value = []
    }
    loaded.value = true
  }

  /** 写回索引（失败不抛出——项目仍在内存可用，下次变更会重试） */
  async function persist(): Promise<void> {
    try {
      await fsService.writeText(projectsIndexFileSync(), JSON.stringify(projects.value, null, 2))
    } catch {
      // 持久化失败不打断用户操作；内存态保持最新
    }
  }

  /**
   * 加入项目（打开文件夹后调用）：重复路径复用已有项目并更新 lastOpenedAt；
   * 返回最终项目（供调用方展开/建会话）。
   */
  function addProject(rootPath: string, name: string): Project {
    const now = Date.now()
    const existing = findByRootPath(rootPath)
    if (existing) {
      existing.lastOpenedAt = now
      void persist()
      return existing
    }
    const project: Project = {
      id: projectIdFromPath(rootPath),
      name,
      rootPath,
      addedAt: now,
      lastOpenedAt: now,
    }
    // 极端情况下 hash 碰撞：退化用随机 ID 保证唯一
    if (byId.value.has(project.id)) project.id = createId('proj')
    projects.value.push(project)
    void persist()
    return project
  }

  /** 移除项目：仅删索引，不删磁盘文件（design D4） */
  function removeProject(projectId: string): void {
    const idx = projects.value.findIndex((p) => p.id === projectId)
    if (idx !== -1) projects.value.splice(idx, 1)
    void persist()
  }

  /** 打开/激活项目时刷新 lastOpenedAt（「最近」分组排序依据） */
  function touchProject(projectId: string): void {
    const p = get(projectId)
    if (p) {
      p.lastOpenedAt = Date.now()
      void persist()
    }
  }

  return {
    projects,
    loaded,
    recentProjects,
    get,
    findByRootPath,
    load,
    addProject,
    removeProject,
    touchProject,
  }
})
