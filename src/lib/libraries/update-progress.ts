import type { LibraryUpdateSkillStatus } from '@/bindings';
import type { SkillUpdateDisplayStatus } from '@/stores/skills-utils';

export type LibraryUpdatePhase = 'idle' | 'checking' | 'preparing' | 'ready' | 'executing';

/**
 * 把整库更新的批次阶段投影到每个成员的展示状态。
 *
 * 与 `Skills` 页同一个模型：批次内的成员共享当前阶段，因此卡片能各自显示进度条；执行结束后
 * 按后端返回的逐成员结果切换为完成或失败。库成员在后端本来就是逐个条件提交，所以"哪些成员
 * 在本次批次里"和"每个成员的结果"都是真实信息，不是估算。
 */
export function libraryUpdateDisplayStatuses(
  phase: LibraryUpdatePhase,
  activeSkillNames: readonly string[],
  lastResults: Readonly<Record<string, LibraryUpdateSkillStatus>>,
): Record<string, SkillUpdateDisplayStatus> {
  const statuses: Record<string, SkillUpdateDisplayStatus> = {};

  if (phase === 'preparing' || phase === 'executing') {
    // preparing 阶段在取来源内容，executing 阶段在写入成员目录。
    const active: SkillUpdateDisplayStatus = phase === 'preparing' ? 'acquiring' : 'updating';
    for (const name of activeSkillNames) statuses[name] = active;
    return statuses;
  }

  for (const [name, result] of Object.entries(lastResults)) {
    statuses[name] = result === 'succeeded' ? 'done' : 'failed';
  }
  return statuses;
}
