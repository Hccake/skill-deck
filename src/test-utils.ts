// src/test-utils.ts
// Mock @tauri-apps/api/core to prevent Tauri runtime errors in test environment
import { vi } from 'vitest';

// Mock the tauri invoke mechanism used by bindings.ts
vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
  Channel: vi.fn(),
}));

// Mock i18next to avoid initialization issues in tests
vi.mock('@/i18n', () => ({
  default: {
    t: (key: string) => key,
    changeLanguage: vi.fn(),
  },
}));
