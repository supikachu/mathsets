/**
 * 高保真数学公式图像压缩引擎
 *
 * 补丁九：禁止 JPEG（导致数学上下标振铃伪影），使用 WebP 0.95 或 PNG 无损
 * 补丁十四：压缩后超 9MB 时二次退避为 JPEG 0.88 + 1500px
 */

/** 长边上限（补丁九：从 1500px 提升至 2000px，确保公式边缘锐利） */
const MAX_LONG_EDGE = 2000

/** WebP 压缩质量 */
const WEBP_QUALITY = 0.95

/** 二次退避长边 */
const FALLBACK_LONG_EDGE = 1500

/** 二次退避 JPEG 质量 */
const FALLBACK_JPEG_QUALITY = 0.88

/** 后端 Axum body limit 为 10MB，安全阈值 9MB */
const SIZE_LIMIT = 9 * 1024 * 1024

/** 图片加载超时时间（毫秒） — 防止大图或损坏文件导致永久挂起 */
const LOAD_IMAGE_TIMEOUT_MS = 30000

/** 将 File/Image 加载为 HTMLImageElement（含超时保护） */
function loadImage(src: string): Promise<HTMLImageElement> {
  return new Promise((resolve, reject) => {
    const img = new Image()
    let settled = false

    const timer = setTimeout(() => {
      if (!settled) {
        settled = true
        img.src = '' // 中断加载
        reject(new Error('图片加载超时（30s），文件可能过大或已损坏'))
      }
    }, LOAD_IMAGE_TIMEOUT_MS)

    img.onload = () => {
      if (!settled) {
        settled = true
        clearTimeout(timer)
        resolve(img)
      }
    }
    img.onerror = () => {
      if (!settled) {
        settled = true
        clearTimeout(timer)
        reject(new Error('图片加载失败，文件可能已损坏'))
      }
    }
    img.src = src
  })
}

/** Canvas 绘制 + 导出 Blob */
async function canvasToBlob(
  img: HTMLImageElement,
  maxLongEdge: number,
  mimeType: string,
  quality?: number,
): Promise<Blob> {
  // 等比缩放，长边不超过 maxLongEdge
  let w = img.naturalWidth
  let h = img.naturalHeight
  if (w > maxLongEdge || h > maxLongEdge) {
    if (w >= h) {
      h = Math.round((h / w) * maxLongEdge)
      w = maxLongEdge
    } else {
      w = Math.round((w / h) * maxLongEdge)
      h = maxLongEdge
    }
  }

  const canvas = document.createElement('canvas')
  canvas.width = w
  canvas.height = h
  const ctx = canvas.getContext('2d')!

  // 白底填充（防止透明 PNG 在 JPEG/WebP 转换时变黑）
  ctx.fillStyle = '#FFFFFF'
  ctx.fillRect(0, 0, w, h)

  // 高质量绘制
  ctx.imageSmoothingEnabled = true
  ctx.imageSmoothingQuality = 'high'
  ctx.drawImage(img, 0, 0, w, h)

  return new Promise((resolve, reject) => {
    canvas.toBlob(
      (blob) => {
        // 释放 canvas 内存
        canvas.width = 0
        canvas.height = 0
        if (blob) {
          resolve(blob)
        } else {
          reject(new Error('Canvas 压缩失败'))
        }
      },
      mimeType,
      quality,
    )
  })
}

/**
 * 压缩图片文件
 *
 * 主压缩：WebP 0.95（或 PNG 无损降级），长边 ≤ 2000px
 * 二次退避（补丁十四）：若结果 > 9MB，降级为 JPEG 0.88 + 1500px
 */
export async function compressImage(file: File | Blob): Promise<Blob> {
  const dataUrl = URL.createObjectURL(file)
  try {
    const img = await loadImage(dataUrl)

    // 主压缩：WebP 0.95（高保真，数学公式优先）
    let blob: Blob
    try {
      blob = await canvasToBlob(img, MAX_LONG_EDGE, 'image/webp', WEBP_QUALITY)
    } catch {
      // WebP 不支持时降级为 PNG 无损
      blob = await canvasToBlob(img, MAX_LONG_EDGE, 'image/png')
    }

    // ⚠️ 补丁十四：压缩后体积超 9MB 时二次退避
    if (blob.size > SIZE_LIMIT) {
      console.warn(
        `图片压缩后 ${Math.round(blob.size / 1024 / 1024)}MB 超 9MB 限制，执行二次退避`,
      )
      try {
        blob = await canvasToBlob(
          img,
          FALLBACK_LONG_EDGE,
          'image/jpeg',
          FALLBACK_JPEG_QUALITY,
        )
      } catch {
        // JPEG 也失败时使用 PNG 降级尺寸
        blob = await canvasToBlob(img, FALLBACK_LONG_EDGE, 'image/png')
      }
    }

    return blob
  } finally {
    URL.revokeObjectURL(dataUrl)
  }
}

/** 将 Blob 转为 File（用于 FormData 上传） */
export function blobToFile(blob: Blob, filename = 'image.webp'): File {
  return new File([blob], filename, { type: blob.type })
}
