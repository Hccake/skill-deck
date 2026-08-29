import { describe, expect, it } from 'vitest';
import type { LibraryUsage, LibraryUsageProjection } from '@/bindings';
import { partitionLibraryUsages, summarizeLibraryUsage } from '../usage-presentation';

const projection: LibraryUsageProjection[] = [
  { libraryId: 'applied', confirmedCount: 2, pendingCount: 0 },
  { libraryId: 'incoming', confirmedCount: 0, pendingCount: 1 },
  { libraryId: 'both', confirmedCount: 1, pendingCount: 2 },
];

const usage = (projectId: string, state: LibraryUsage['state']): LibraryUsage => ({
  context: {
    environment: { kind: 'native' },
    scope: { scope: 'project', project_id: projectId },
  },
  project: {
    id: projectId,
    nativePath: `/work/${projectId}`,
    displayName: null,
    order: null,
    suppressCrossStorageWarning: false,
  },
  state,
});

describe('summarizeLibraryUsage', () => {
  it('reports a library that no location references as unapplied', () => {
    expect(summarizeLibraryUsage(projection, 'absent')).toEqual({
      confirmedCount: 0,
      pendingCount: 0,
      applied: false,
      pendingAdjustment: false,
    });
  });

  it('keeps confirmed and pending counts separate', () => {
    expect(summarizeLibraryUsage(projection, 'applied')).toMatchObject({
      confirmedCount: 2,
      applied: true,
      pendingAdjustment: false,
    });
    expect(summarizeLibraryUsage(projection, 'incoming')).toMatchObject({
      confirmedCount: 0,
      applied: false,
      pendingAdjustment: true,
    });
    expect(summarizeLibraryUsage(projection, 'both')).toMatchObject({
      confirmedCount: 1,
      pendingCount: 2,
      applied: true,
      pendingAdjustment: true,
    });
  });

  it('treats a missing projection as no usage', () => {
    expect(summarizeLibraryUsage(undefined, 'applied').applied).toBe(false);
  });
});

describe('partitionLibraryUsages', () => {
  it('separates confirmed locations from locations awaiting an unfinished adjustment', () => {
    const { confirmed, pending } = partitionLibraryUsages([
      usage('global', 'confirmed'),
      usage('skill-deck', 'pendingAdjustment'),
      usage('my-web-app', 'confirmed'),
    ]);

    expect(confirmed.map((item) => item.context.scope)).toEqual([
      { scope: 'project', project_id: 'global' },
      { scope: 'project', project_id: 'my-web-app' },
    ]);
    expect(pending.map((item) => item.context.scope)).toEqual([
      { scope: 'project', project_id: 'skill-deck' },
    ]);
  });
});
