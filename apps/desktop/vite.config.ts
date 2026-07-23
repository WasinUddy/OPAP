import react from '@vitejs/plugin-react';
import { readFileSync } from 'node:fs';
import { defineConfig } from 'vitest/config';

const copyingModuleId = 'virtual:opap-copying';
const resolvedCopyingModuleId = `\0${copyingModuleId}`;

export default defineConfig({
  plugins: [
    {
      name: 'opap-offline-license',
      enforce: 'pre',
      resolveId(source) {
        return source === copyingModuleId ? resolvedCopyingModuleId : undefined;
      },
      load(id) {
        if (id !== resolvedCopyingModuleId) return undefined;
        const copying = readFileSync(new URL('../../COPYING', import.meta.url), 'utf8');
        return `export default ${JSON.stringify(copying)};`;
      },
    },
    react(),
  ],
  test: {
    environment: 'jsdom',
    globals: true,
    setupFiles: './src/test/setup.ts',
    css: true,
  },
});
