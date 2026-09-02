import { svelte } from '@sveltejs/vite-plugin-svelte'
import { defineConfig } from 'vite'

// Tauri devUrl expects :1420 (see src-tauri/tauri.conf.json); strictPort so a
// busy port fails loudly instead of silently drifting to 1421.
export default defineConfig({
  plugins: [svelte()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
  },
  build: {
    target: 'es2022',
  },
})
