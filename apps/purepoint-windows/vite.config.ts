/// <reference types="vitest/config" />
import { fileURLToPath } from 'node:url';
import react from '@vitejs/plugin-react';
import { defineConfig } from 'vite';

const uiRoot = fileURLToPath(new URL('./ui', import.meta.url));
// Tests read the macOS theme sources and docs/reference from outside the app (WIN-5.8).
const repoRoot = fileURLToPath(new URL('../..', import.meta.url));
const outDir = fileURLToPath(new URL('./dist', import.meta.url));
// The Point Guard chat tests run the real bridge (apps/purepoint-mobile/bridge) in-process
// (WIN-9.4); its one package import, `ws`, resolves to this app's copy.
const wsPackage = fileURLToPath(new URL('./node_modules/ws/wrapper.mjs', import.meta.url));

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
    ...(process.env.VITEST ? { fs: { allow: [repoRoot] } } : {}),
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
    // Keep tokens.css real so tokens.test.ts can read it with ?raw (Vitest empties CSS by default).
    css: { include: [/tokens.css/] },
    alias: [{ find: /^ws$/, replacement: wsPackage }],
  },
});
