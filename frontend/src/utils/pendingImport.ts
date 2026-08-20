/**
 * 题库页 → 编辑页之间传递待导入文件（File 无法走路由 state / sessionStorage）。
 * take* 为一次性消费，避免返回列表后再进编辑页重复触发。
 */

let pendingFile: File | null = null
let pendingOpen = false

export function isImportableFile(file: File): boolean {
  return file.type === 'application/pdf' || /\.pdf$/i.test(file.name) || file.type.startsWith('image/')
}

export function fileFromClipboard(e: ClipboardEvent): File | null {
  const items = e.clipboardData?.items
  if (!items) return null
  for (const item of items) {
    if (item.kind !== 'file') continue
    const file = item.getAsFile()
    if (file && isImportableFile(file)) return file
  }
  const files = e.clipboardData?.files
  if (files) {
    for (const file of files) {
      if (isImportableFile(file)) return file
    }
  }
  return null
}

export function setPendingImportFile(file: File) {
  pendingFile = file
  pendingOpen = true
}

export function takePendingImportFile(): File | null {
  const file = pendingFile
  pendingFile = null
  return file
}

export function setPendingImportOpen(open = true) {
  pendingOpen = open
}

export function takePendingImportOpen(): boolean {
  const open = pendingOpen
  pendingOpen = false
  return open
}
