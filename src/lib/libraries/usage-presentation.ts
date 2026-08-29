import type { LibraryId, LibraryUsage, LibraryUsageProjection } from '@/bindings';
import { environmentKey } from '@/lib/context';
import { registeredProjectDisplayName } from '@/lib/projects/presentation';

export function libraryUsageDisplayName(
  usage: LibraryUsage,
  globalLabel: string,
): string {
  if (usage.context.scope.scope === 'global') return globalLabel;
  return usage.project
    ? registeredProjectDisplayName(usage.project)
    : usage.context.scope.project_id;
}

export function libraryUsageIdentityKey(usage: LibraryUsage): string {
  const scope = usage.context.scope;
  const location = scope.scope === 'global' ? 'global' : `project:${scope.project_id}`;
  return `${environmentKey(usage.context.environment)}:${location}:${usage.state}`;
}

/**
 * 侧栏副行需要表达的三种状态。
 *
 * 生效与锁定是两件事：`confirmedCount` 表示配置已经起作用，`pendingCount` 表示只有未完成的
 * 应用操作引用该库。两者都为 0 时该库未应用。
 */
export interface LibraryUsageSummary {
  confirmedCount: number;
  pendingCount: number;
  applied: boolean;
  pendingAdjustment: boolean;
}

const EMPTY_SUMMARY: LibraryUsageSummary = {
  confirmedCount: 0,
  pendingCount: 0,
  applied: false,
  pendingAdjustment: false,
};

/**
 * 没有任何位置引用的库不会出现在投影中，按缺失即 0 处理。
 */
export function summarizeLibraryUsage(
  projection: readonly LibraryUsageProjection[] | undefined,
  libraryId: LibraryId,
): LibraryUsageSummary {
  const entry = projection?.find((item) => item.libraryId === libraryId);
  if (!entry) return EMPTY_SUMMARY;
  return {
    confirmedCount: entry.confirmedCount,
    pendingCount: entry.pendingCount,
    applied: entry.confirmedCount > 0,
    pendingAdjustment: entry.pendingCount > 0,
  };
}

/**
 * 主区 header 需要按状态分流展示位置，已确认生效的排在前面。
 */
export function partitionLibraryUsages(usages: readonly LibraryUsage[]): {
  confirmed: LibraryUsage[];
  pending: LibraryUsage[];
} {
  const confirmed: LibraryUsage[] = [];
  const pending: LibraryUsage[] = [];
  for (const usage of usages) {
    if (usage.state === 'confirmed') confirmed.push(usage);
    else pending.push(usage);
  }
  return { confirmed, pending };
}
