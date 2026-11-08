import {defineConfig} from 'vite'
import vue from '@vitejs/plugin-vue'
import glsl from 'vite-plugin-glsl'
import {viteSingleFile} from 'vite-plugin-singlefile'
export default defineConfig({
  plugins: [glsl(), vue(), viteSingleFile()],
  build: {target: 'es2022', assetsInlineLimit: 100000000},
  define: {'process.env.PROMISE_QUEUE_COVERAGE': false},
})
