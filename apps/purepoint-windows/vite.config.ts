/// <reference types="vitest/config" />
import { fileURLToPath } from 'node:url';
import react from '@vitejs/plugin-react';
import { defineConfig } from 'vite';

const uiRoot = fileURLToPath(new URL('./ui', import.meta.url));
const outDir = fileURLToPath(new URL('./dist', import.meta.url));

// Tauri loads the dev server from a fixed port and the production bundle from
// ../dist (see src-tauri/tauri.conf.json); keep the two in step.
export default defineConfig({
  root: uiRoot,
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ['**/src-tauri/**'] },
  },
  envPrefix: ['VITE_', 'TAURI_ENV_'],
  build: {
    outDir,
    emptyOutDir: true,
    // WebView2 is Chromium-based and evergreen on Windows 11 (DEC-12).
    target: 'chrome120',
    sourcemap: !!process.env.TAURI_ENV_DEBUG,
  },
  test: {
    root: uiRoot,
    environment: 'jsdom',
    globals: true,
    setupFiles: ['./src/test/setup.ts'],
  },
});
