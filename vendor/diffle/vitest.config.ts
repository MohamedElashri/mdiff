import { fileURLToPath } from 'node:url';
import { defineConfig } from 'vitest/config';
import pkg from './package.json' with { type: 'json' };

// Mirrors `paths` in the tsconfigs: vite's own tsconfig lookup skips files a config excludes.
const srcRoot = fileURLToPath(new URL('./src', import.meta.url));

export default defineConfig({
  define: { __DIFFLE_VERSION__: JSON.stringify(pkg.version) },
  resolve: { alias: { '@': srcRoot } },
  test: {
    include:
      process.env.MDIFF_TEST_TOOLING === '1'
        ? ['test/scripts/**/*.test.ts']
        : [
            'test/client/**/*.test.ts',
            'test/shared/**/*.test.ts',
            'test/desktop/**/*.test.ts',
            'test/fixture/**/*.test.ts',
            'test/scripts/protocol-schema.test.ts',
          ],
    maxWorkers: 4,
    environment: 'node',
    // Bound worker memory on developer laptops and CI; keep rendering regressions in the default suite.
    testTimeout: process.platform === 'win32' ? 30_000 : 5_000,
  },
});
