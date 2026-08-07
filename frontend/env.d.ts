/// <reference types="vite/client" />

declare module '*.vue' {
  import type { DefineComponent } from 'vue'
  const component: DefineComponent<{}, {}, any>
  export default component
}

// ============================================================
// cropperjs v2 Web Components 类型声明
// ------------------------------------------------------------
// cropperjs v2 采用 Custom Elements 架构，import 'cropperjs' 会自动注册
// <cropper-canvas> / <cropper-image> / <cropper-selection> 等自定义元素。
// 这里声明它们的实例方法，避免 vue-tsc 报 unknown property 错误。
// ============================================================

interface CropperSelectionElement extends HTMLElement {
  $toCanvas(): Promise<HTMLCanvasElement>
}

interface CropperImageElement extends HTMLElement {
  $ready(): Promise<CropperImageElement>
  $transform(matrix: number[]): Promise<CropperImageElement>
}
