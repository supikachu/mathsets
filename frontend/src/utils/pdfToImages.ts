/**
 * PDF 逐页渲染为图片（async generator）
 *
 * 补丁四：动态精度计算 — safeScale = max(1.5, min(2.0, 2000 / width))
 * 补丁十三：PDF 30 页物理硬限制 — 超限时截断 + onTruncated 回调
 *
 * 使用 pdfjs-dist（Firefox PDF Viewer 核心库），零原生依赖。
 */

import * as pdfjsLib from 'pdfjs-dist'
// Vite 会将 ?url 后缀的 import 解析为文件 URL
import workerSrc from 'pdfjs-dist/build/pdf.worker.min.mjs?url'

// 配置 Worker
pdfjsLib.GlobalWorkerOptions.workerSrc = workerSrc

/** PDF 页数物理硬限制（补丁十三：防 OOM） */
const PDF_PAGE_HARD_LIMIT = 30

/** 渲染 Canvas 最大宽度（补丁四） */
const MAX_CANVAS_WIDTH = 2000

/** 最小缩放比（补丁四：保证公式清晰度） */
const MIN_SCALE = 1.5

/** 最大缩放比（补丁四） */
const MAX_SCALE = 2.0

/** 单页渲染结果 */
export interface PdfPageImage {
  page: number
  dataUrl: string
  total: number
}

/** 渲染选项 */
export interface PdfToImagesOptions {
  /** 进度回调 */
  onProgress?: (current: number, total: number) => void
  /** 页数被截断时的回调（补丁十三） */
  onTruncated?: (originalPages: number, actualPages: number) => void
}

/**
 * 将 PDF 文件逐页渲染为图片
 *
 * 使用动态 safeScale 确保渲染出的 Canvas 宽度不超过 2000px，
 * 同时最小 scale 不低于 1.5 以保证公式清晰度。
 *
 * 超过 30 页时物理截断（补丁十三）。
 */
export async function* pdfToImages(
  file: File,
  options?: PdfToImagesOptions,
): AsyncGenerator<PdfPageImage, void, unknown> {
  const arrayBuffer = await file.arrayBuffer()
  const pdf = await pdfjsLib.getDocument({ data: arrayBuffer }).promise

  const originalPages = pdf.numPages
  // ⚠️ 补丁十三：强制截断为 30 页
  const total = Math.min(originalPages, PDF_PAGE_HARD_LIMIT)

  if (originalPages > PDF_PAGE_HARD_LIMIT) {
    options?.onTruncated?.(originalPages, total)
  }

  try {
    for (let i = 1; i <= total; i++) {
      const page = await pdf.getPage(i)

      // ⚠️ 补丁四：动态精度计算
      const unscaledViewport = page.getViewport({ scale: 1.0 })
      const safeScale = Math.max(
        MIN_SCALE,
        Math.min(MAX_SCALE, MAX_CANVAS_WIDTH / unscaledViewport.width),
      )
      const viewport = page.getViewport({ scale: safeScale })

      const canvas = document.createElement('canvas')
      const ctx = canvas.getContext('2d')!
      canvas.width = viewport.width
      canvas.height = viewport.height

      await page.render({ canvasContext: ctx, viewport }).promise

      const dataUrl = canvas.toDataURL('image/jpeg', 0.85)

      // 释放 canvas 内存
      canvas.width = 0
      canvas.height = 0

      // 释放 pdfjs 页面缓存
      page.cleanup()

      yield { page: i, dataUrl, total }

      options?.onProgress?.(i, total)
    }
  } finally {
    await pdf.destroy()
  }
}
