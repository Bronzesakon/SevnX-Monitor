import { createApp } from 'vue'
import { createPinia } from 'pinia'
import App from './App.vue'
import './style.css'

export function mountApplication(): void {
  const app = createApp(App)
  app.use(createPinia())
  app.mount('#app')
}
