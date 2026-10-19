/// <reference types="vitest/config" />
import { svelte } from '@sveltejs/vite-plugin-svelte'
import tailwindcss from '@tailwindcss/vite'
import { defineConfig } from 'vite'

// During `npm run dev`, API calls go to a backend started with
// `cargo run -- --simulate --listen 127.0.0.1:9495`.
const backend = process.env.BACKEND ?? 'http://127.0.0.1:9495'

export default defineConfig({
  plugins: [tailwindcss(), svelte()],
  server: {
    proxy: {
      '/api/live': { target: backend, ws: true },
      '/api': backend,
      '/metrics': backend,
    },
  },
  build: { chunkSizeWarningLimit: 300 },
  test: { environment: 'node', include: ['src/**/*.test.ts'] },
})
