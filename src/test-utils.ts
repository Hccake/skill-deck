// src/test-utils.ts
// Mock @tauri-apps/api/core to prevent Tauri runtime errors in test environment
import { vi } from 'vitest';
import type {
  AgentSelectionSnapshot,
  AgentRuntimeSnapshot,
  AgentSource,
  DetectionState,
  ResolvedAgent,
  ResolvedAgentScope,
} from '@/bindings';

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

if (typeof globalThis.ResizeObserver === 'undefined') {
  globalThis.ResizeObserver = class ResizeObserver {
    observe() {}
    unobserve() {}
    disconnect() {}
  };
}

export function makeResolvedAgentScope(
  target: Partial<ResolvedAgentScope> = {},
): ResolvedAgentScope {
  return {
    enabled: target.enabled ?? true,
    readsStandard: target.readsStandard ?? true,
    standardPath: target.standardPath ?? '~/.agents/skills',
    privatePath: target.privatePath ?? null,
    readPaths: target.readPaths ?? [target.standardPath ?? '~/.agents/skills'],
    standardPresence: target.standardPresence ?? 'missing',
    privatePresence: target.privatePresence ?? null,
    legacyPaths: target.legacyPaths ?? [],
  };
}

export function makeResolvedScopeFixture(target: {
  path: string;
  automatic?: boolean;
  supported?: boolean;
  defaultAvailable?: boolean;
  standardPath?: string;
  readPaths?: string[];
  privatePath?: string | null;
  availability?: string;
}): ResolvedAgentScope {
  const readsStandard = target.defaultAvailable ?? target.automatic ?? false;
  const standardPath = target.standardPath
    ?? (readsStandard ? target.path : '~/.agents/skills');
  const privatePath = target.privatePath
    ?? (readsStandard ? null : target.path);
  return makeResolvedAgentScope({
    enabled: target.supported ?? true,
    readsStandard,
    standardPath,
    privatePath,
    readPaths: target.readPaths ?? [target.path],
  });
}

export function makeResolvedAgent(options: {
  id: string;
  displayName?: string;
  source?: AgentSource;
  detection?: DetectionState;
  global?: Partial<ResolvedAgentScope>;
  project?: Partial<ResolvedAgentScope>;
}): ResolvedAgent {
  const global = makeResolvedAgentScope(options.global);
  const project = makeResolvedAgentScope(options.project);
  return {
    definition: {
      id: options.id,
      displayName: options.displayName ?? options.id,
      source: options.source ?? 'builtin',
      aliases: [],
      global: {
        enabled: global.enabled,
        readsStandard: global.readsStandard,
        privatePath: global.privatePath
          ? { kind: 'home', relativePath: `.${options.id}/skills` }
          : null,
      },
      project: {
        enabled: project.enabled,
        readsStandard: project.readsStandard,
        privatePath: project.privatePath
          ? { kind: 'project', relativePath: `.${options.id}/skills` }
          : null,
      },
      detection: {
        kind: 'anyPathExists',
        paths: [{ kind: 'home', relativePath: `.${options.id}` }],
      },
      legacyPaths: [],
      adapter: 'standard',
    },
    detection: options.detection ?? 'detected',
    detectionReason: null,
    global,
    project,
  };
}

export function makeAgentRuntimeSnapshot(
  agents: ResolvedAgent[],
): AgentRuntimeSnapshot {
  return {
    registryRevision: 'registry-1',
    environmentRevision: 'environment-1',
    environment: { kind: 'native' },
    availability: 'available',
    projectPath: null,
    agents: Object.fromEntries(
      agents.map((agent) => [agent.definition.id, agent]),
    ),
  };
}

export function makeAgentSelectionSnapshot(
  overrides: Partial<AgentSelectionSnapshot> = {},
): AgentSelectionSnapshot {
  return {
    agents: [],
    installOptions: [],
    groups: [],
    initialSelectedOptionIds: [],
    unavailableExplicitAgents: [],
    userModeOptionIds: [],
    revision: 'selection-revision-1',
    ...overrides,
  };
}
