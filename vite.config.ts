import tailwindcss from '@tailwindcss/vite';
import react from '@vitejs/plugin-react';
import { readFileSync } from 'fs';
import path from 'path';
import { defineConfig } from 'vite';

// The version shown in the title bar comes from tauri.conf.json at build time, so it changes with every release.
const IVY_VERSION = JSON.parse(readFileSync(path.resolve(__dirname, 'src-tauri/tauri.conf.json'), 'utf-8')).version;

export default defineConfig({
  plugins: [react(), tailwindcss()],
  define: { __IVY_VERSION__: JSON.stringify(IVY_VERSION) },
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
    },
  },
  build: {
    rollupOptions: {
      input: {
        main: path.resolve(__dirname, 'index.html'),
        capsule: path.resolve(__dirname, 'capsule.html'),
      },
    },
  },
  server: {
    port: 3000,
    strictPort: true,
    hmr: process.env.DISABLE_HMR !== 'true',
    watch: process.env.DISABLE_HMR === 'true' ? null : {},
  },
});
