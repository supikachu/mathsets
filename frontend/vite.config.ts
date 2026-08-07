import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'
import { fileURLToPath } from 'node:url'

export default defineConfig({
  plugins: [
    vue({
      // cropperjs v2 使用 Web Components（cropper-canvas / cropper-image 等）
      // 告知 Vue 模板编译器：cropper-* 前缀的标签是自定义元素，不要发出告警
      template: {
        compilerOptions: {
          isCustomElement: (tag: string) => tag.startsWith('cropper-'),
        },
      },
    }),
  ],
  resolve: {
    alias: {
      '@': fileURLToPath(new URL('./src', import.meta.url)),
    },
  },
  server: {
    port: 5173,
    proxy: {
      '/api': {
        target: 'http://127.0.0.1:3000',
        changeOrigin: true,
      },
      // 用户头像等上传文件 — 直接由后端 ServeDir 提供
      '/uploads': {
        target: 'http://127.0.0.1:3000',
        changeOrigin: true,
      },
    },
  },
})
