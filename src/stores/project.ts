// 项目（工作区）仓库：文件树与 Mock 内存文件内容。
// 项目分两类：文件夹项目（用户打开的目录）与临时会话目录（~/.lightr-vibe/tmpsession）。
// 后续可替换为 Tauri fs API 读取真实目录。

import { defineStore } from 'pinia'
import { ref } from 'vue'
import type { Project } from '../types/session'

/** 临时会话目录项目（软件数据目录下的 tmpsession） */
export const TMP_PROJECT_ID = 'proj-tmp'

const mockProjects: Project[] = [
  {
    id: 'proj-29028',
    name: '29028',
    root: [
      { name: '.ccgui', type: 'dir', children: [{ name: 'config.json', type: 'file' }] },
      { name: 'snake.html', type: 'file' },
      { name: 'super-mario.html', type: 'file' },
      { name: '春天作文.txt', type: 'file' },
    ],
    files: {
      '.ccgui/config.json': '{\n  "theme": "light",\n  "language": "zh-CN"\n}\n',
      'snake.html':
        '<!doctype html>\n<html lang="zh-CN">\n  <head>\n    <meta charset="UTF-8" />\n    <title>贪吃蛇</title>\n  </head>\n  <body>\n    <canvas id="board" width="400" height="400"></canvas>\n    <script>\n      // 使用方向键控制，撞墙或撞到自己则重新开始\n    </script>\n  </body>\n</html>\n',
      'super-mario.html':
        '<!doctype html>\n<html lang="zh-CN">\n  <head>\n    <meta charset="UTF-8" />\n    <title>Super Mario</title>\n  </head>\n  <body>\n    <div id="game"></div>\n  </body>\n</html>\n',
      '春天作文.txt': '春天来了，万物复苏。\n小草从土里探出头来，桃花也开了。\n',
    },
  },
  {
    id: TMP_PROJECT_ID,
    name: 'tmpsession',
    root: [],
    files: {},
  },
]

export const useProjectStore = defineStore('project', () => {
  const projects = ref<Project[]>(mockProjects)

  /** 文件夹项目（不含临时会话目录） */
  const folderProjects = () => projects.value.filter((p) => p.id !== TMP_PROJECT_ID)

  function get(projectId: string | null | undefined): Project | null {
    return projects.value.find((p) => p.id === projectId) ?? null
  }

  function getFileContent(projectId: string, path: string): string {
    return get(projectId)?.files[path] ?? ''
  }

  function setFileContent(projectId: string, path: string, content: string) {
    const p = get(projectId)
    if (p) p.files[path] = content
  }

  return { projects, folderProjects, get, getFileContent, setFileContent }
})
