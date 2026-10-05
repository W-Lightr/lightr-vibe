// 文件类型工具：图标语义与 Monaco 语言映射

export function languageByPath(path: string): string {
  const ext = path.split('.').pop()?.toLowerCase() ?? ''
  switch (ext) {
    case 'html':
    case 'htm':
      return 'html'
    case 'css':
    case 'scss':
    case 'less':
      return 'css'
    case 'ts':
    case 'tsx':
    case 'js':
    case 'jsx':
      return 'typescript'
    case 'json':
      return 'json'
    case 'md':
      return 'markdown'
    case 'vue':
      return 'html'
    default:
      return 'plaintext'
  }
}

/** 文件图标类别：code / text / file */
export function fileIconKind(path: string): 'code' | 'text' | 'file' {
  const ext = path.split('.').pop()?.toLowerCase() ?? ''
  if (['html', 'htm', 'ts', 'tsx', 'js', 'jsx', 'vue', 'css', 'json'].includes(ext)) return 'code'
  if (['txt', 'md'].includes(ext)) return 'text'
  return 'file'
}
