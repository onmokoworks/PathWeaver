import {createApp} from 'vue'
import {createPinia} from 'pinia'
import './icons'
import App from './App.vue'
import {send} from './protocol'
const report = (error: unknown) => send({type: 'error', message: (error instanceof Error ? `${error.name}: ${error.message}` : 'UI runtime error').slice(0, 240)})
window.addEventListener('error', event => report(event.error))
window.addEventListener('unhandledrejection', event => report(event.reason))
const app = createApp(App).use(createPinia())
app.config.errorHandler = report
app.mount('#app')
