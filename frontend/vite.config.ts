import { defineConfig } from 'vite'
import { fileURLToPath, URL } from 'node:url'
import vue from '@vitejs/plugin-vue'

const host = process.env.TAURI_DEV_HOST

export default defineConfig(({ command }) => ({
  plugins: [vue()],
  clearScreen: false,
  resolve: {
    // Browser fixtures exist only in the Vite serve graph, never in a package build.
    alias: {
      '@runtime': fileURLToPath(new URL(
        command === 'serve' ? './src/runtime.development.ts' : './src/runtime.production.ts',
        import.meta.url,
      )),
    },
  },
  server: {
    host: host || '127.0.0.1',
    port: 1420,
    strictPort: true,
  },
  envPrefix: ['VITE_', 'TAURI_'],
  build: {
    target: process.env.TAURI_ENV_PLATFORM === 'windows' ? 'chrome105' : 'es2020',
    minify: process.env.TAURI_ENV_DEBUG ? false : 'esbuild',
    sourcemap: !!process.env.TAURI_ENV_DEBUG,
    chunkSizeWarningLimit: 550,
    rollupOptions: {
      output: {
        manualChunks(id) {
          if (id.includes('/node_modules/echarts/')) return 'echarts'
          if (id.includes('/node_modules/vue/') || id.includes('/node_modules/pinia/')) return 'vue-vendor'
        },
      },
    },
  },
}))
