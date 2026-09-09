import { defineConfig } from 'vite';

export default defineConfig({
  root: 'src',
  clearScreen: false,
  build: {
    outDir: '../dist',
    emptyOutDir: true,
    target: 'chrome105'
  },
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ['**/src-tauri/**'] }
  }
});
