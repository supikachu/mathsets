/**
 * 识别原稿（PDF / 图片）缓存。
 * sessionStorage 放不下文件，离开新建页后再恢复草稿时用 IndexedDB 回填左侧原文。
 */
import { get, set, del } from 'idb-keyval'

const SOURCE_KEY = 'q-ai-source-new'

export interface AiSourceRecord {
  name: string
  type: string
  kind: 'pdf' | 'image'
  blob: Blob
  savedAt: number
}

export async function saveAiSourceFile(file: File, kind: 'pdf' | 'image'): Promise<void> {
  try {
    await set(SOURCE_KEY, {
      name: file.name,
      type: file.type || (kind === 'pdf' ? 'application/pdf' : file.type),
      kind,
      blob: file,
      savedAt: Date.now(),
    } satisfies AiSourceRecord)
  } catch (e) {
    console.error('[aiSourceFile] 保存原稿失败:', e)
  }
}

export async function loadAiSourceFile(): Promise<{ file: File; kind: 'pdf' | 'image' } | undefined> {
  try {
    const rec = await get<AiSourceRecord>(SOURCE_KEY)
    if (!rec?.blob) return undefined
    const type = rec.type || rec.blob.type || (rec.kind === 'pdf' ? 'application/pdf' : '')
    const file = new File(
      [rec.blob],
      rec.name || (rec.kind === 'pdf' ? 'original.pdf' : 'original.png'),
      { type },
    )
    return { file, kind: rec.kind === 'pdf' ? 'pdf' : 'image' }
  } catch (e) {
    console.error('[aiSourceFile] 读取原稿失败:', e)
    return undefined
  }
}

export async function clearAiSourceFile(): Promise<void> {
  try {
    await del(SOURCE_KEY)
  } catch (e) {
    console.error('[aiSourceFile] 清除原稿失败:', e)
  }
}
