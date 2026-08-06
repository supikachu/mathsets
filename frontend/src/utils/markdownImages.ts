// ============================================================
// Markdown 图片持久化工具
// ------------------------------------------------------------
// 用途：在 QuestionEdit 保存表单前，将 Markdown 中的临时 blob: URL
//       上传到后端持久化存储，并替换为永久 URL。
//
// 设计要点：
//   1. 单字段内多张图并行上传（Promise.all）
//   2. 同一 blob URL 在同一字段内去重（Set 去重，只上传一次）
//   3. 跨字段共享上传缓存（Map<blobUrl, persistentUrl>），避免
//      同一张图在 stem / solutions / options 中被重复上传
//   4. 上传成功后调用 URL.revokeObjectURL 释放浏览器内存
//   5. 单图上传失败不阻断整体流程：保留 blob URL（下次保存重试），
//      并在控制台记录错误，便于排查
// ============================================================

import { uploadsApi } from '@/api/client'

/** 上传缓存：跨字段共享，避免同一 blob 在多字段中被重复上传 */
export type UploadCache = Map<string, string>

/**
 * 处理 Markdown 文本中的所有 blob: 图片链接：
 *   1. 正则匹配 `![alt](blob:...)`
 *   2. fetch blob URL 取回 Blob 对象
 *   3. 调用 uploadsApi.uploadImage 上传到后端
 *   4. 将 Markdown 中的 blob: URL 替换为持久化 URL
 *
 * @param markdown 原始 Markdown 文本
 * @param cache    跨字段共享的上传缓存（可选，由调用方传入）
 * @returns 替换后的 Markdown 文本
 */
export async function processMarkdownImages(
  markdown: string,
  cache?: UploadCache,
): Promise<string> {
  if (!markdown) return markdown

  // 收集当前字段中所有唯一的 blob: URL
  const blobUrls = new Set<string>()
  // 正则解释：
  //   !\[([^\]]*)\]   —— 捕获 alt 文本（不包含 ]）
  //   \((blob:[^)]+)\) —— 捕获 blob: 开头、不含 ) 的 URL
  const blobUrlRegex = /!\[([^\]]*)\]\((blob:[^)]+)\)/g
  let match: RegExpExecArray | null
  while ((match = blobUrlRegex.exec(markdown)) !== null) {
    blobUrls.add(match[2])
  }

  if (blobUrls.size === 0) return markdown

  // 复用调用方传入的缓存，或在函数内新建
  const cacheMap = cache ?? new Map<string, string>()

  // 待上传列表：跳过缓存中已存在的 blob URL
  const pending = Array.from(blobUrls).filter((url) => !cacheMap.has(url))

  // 并行上传所有未缓存的 blob URL
  if (pending.length > 0) {
    const uploads = pending.map(async (blobUrl) => {
      try {
        // 1. 通过 fetch 将 blob URL 转换回 Blob 对象
        const response = await fetch(blobUrl)
        if (!response.ok) {
          throw new Error(`fetch blob 失败: HTTP ${response.status}`)
        }
        const blob = await response.blob()

        // 2. 从 blob.type 推断扩展名（image/jpeg → jpg，image/png → png，image/webp → webp）
        //    后端会用 infer crate 做 Magic Bytes 校验，客户端类型仅作为 FormData 元信息
        const mimeToExt: Record<string, string> = {
          'image/jpeg': 'jpg',
          'image/png': 'png',
          'image/webp': 'webp',
        }
        const ext = mimeToExt[blob.type] || 'png'
        const file = new File([blob], `image.${ext}`, { type: blob.type || 'image/png' })

        // 3. 调用后端上传接口
        const res = await uploadsApi.uploadImage(file)
        const persistentUrl = res.data.url

        // 4. 释放浏览器内存（blob URL 不可再访问）
        URL.revokeObjectURL(blobUrl)

        return { blobUrl, persistentUrl, ok: true as const }
      } catch (e) {
        // 单图失败不阻断整体流程：保留 blob URL，下次保存时可重试
        console.error('[processMarkdownImages] 上传失败，保留 blob URL:', blobUrl, e)
        return { blobUrl, persistentUrl: null, ok: false as const }
      }
    })

    const results = await Promise.all(uploads)

    // 将成功的上传写入缓存，供后续字段复用
    for (const { blobUrl, persistentUrl, ok } of results) {
      if (ok && persistentUrl) {
        cacheMap.set(blobUrl, persistentUrl)
      }
    }
  }

  // 用 split/join 替换 markdown 中的 blob URL（避免 String.replaceAll 的特殊字符问题）
  // 注意：blob URL 中不含正则特殊字符，split/join 在此场景下绝对安全
  let processed = markdown
  for (const [blobUrl, persistentUrl] of cacheMap) {
    if (markdown.includes(blobUrl)) {
      processed = processed.split(blobUrl).join(persistentUrl)
    }
  }

  return processed
}

/**
 * 批量处理多个 Markdown 字段（如 stem / solutions / options）：
 *   - 共享同一份上传缓存，避免跨字段重复上传
 *   - 字段间串行执行（避免并发请求过多导致后端 multipart 解析压力）
 *
 * @param markdowns 多个 Markdown 文本组成的数组
 * @returns 替换后的 Markdown 文本数组（与入参顺序一一对应）
 */
export async function processMarkdownImagesBatch(
  markdowns: string[],
): Promise<string[]> {
  const cache: UploadCache = new Map()
  const results: string[] = []
  for (const md of markdowns) {
    results.push(await processMarkdownImages(md, cache))
  }
  return results
}
