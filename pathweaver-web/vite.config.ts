import {defineConfig} from 'vite'
import vue from '@vitejs/plugin-vue'
import glsl from 'vite-plugin-glsl'
import {viteSingleFile} from 'vite-plugin-singlefile'
// One self-contained index.html (JS, CSS, icons and the wasm core inlined)
// so the Mac app needs no server or network.
export default defineConfig({
  plugins: [glsl(), vue(), viteSingleFile()],
  build: {target: 'es2022', assetsInlineLimit: 100000000},
  assetsInclude: ['**/*.wasm'],
  define: {'process.env.PROMISE_QUEUE_COVERAGE': false},
  server: {host: '127.0.0.1', port: 5174},
})
