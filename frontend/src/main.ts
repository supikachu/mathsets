import { createApp } from 'vue'
import { createPinia } from 'pinia'
import 'katex/dist/katex.min.css'
import { initTheme } from '@/composables/useTheme'
import App from './App.vue'
import router from './router'
import './style.css'

initTheme()

const app = createApp(App)
app.config.errorHandler = (err, _instance, info) => {
  console.error('Vue error:', err, '\nInfo:', info)
}
app.use(createPinia())
app.use(router)
app.mount('#app')
