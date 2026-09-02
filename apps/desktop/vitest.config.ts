import { defineConfig } from 'vitest/config'

// The GPU test renderer boots a window per test; 5s is not enough on a busy machine.
export default defineConfig({
  test: { testTimeout: 40_000, hookTimeout: 40_000 },
})
