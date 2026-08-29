import type { SkillUpdateCheckStatus, SkillUpdateInfo } from '@/bindings';

/**
 * 整库检查的结论。
 *
 * `product.md` 要求分别显示有更新、没有更新、无法检查和上游已删除；检查失败时保留上次成功
 * 结论并说明本次未完成。四类全部列出会过长，因此按下面的规则收敛。
 */
export interface LibraryUpdateSummary {
  /** 尚未取得过任何结论。 */
  unchecked: boolean;
  /** 本次检查未完成，展示的是上次成功的结论。 */
  incomplete: boolean;
  updateAvailable: number;
  upToDate: number;
  cannotCheck: number;
  deletedUpstream: number;
}

export interface LibraryUpdateSummaryItem {
  text: string;
  tone: 'neutral' | 'accent' | 'warning';
}

/** 需要用户处理的类别，按展示优先级排列。 */
const ATTENTION_ORDER: readonly SkillUpdateCheckStatus[] = [
  'updateAvailable',
  'deletedUpstream',
  'cannotCheck',
];

/** 最多内联两类，其余合并成一句，避免摘要行溢出。 */
const INLINE_CATEGORY_LIMIT = 2;

export function summarizeLibraryUpdates(
  checks: Readonly<Record<string, SkillUpdateInfo>>,
  hasError: boolean,
): LibraryUpdateSummary {
  const values = Object.values(checks);
  const count = (status: SkillUpdateCheckStatus) => (
    values.filter((check) => check.status === status).length
  );
  return {
    unchecked: values.length === 0,
    incomplete: hasError,
    updateAvailable: count('updateAvailable'),
    upToDate: count('upToDate'),
    cannotCheck: count('cannotCheck'),
    deletedUpstream: count('deletedUpstream'),
  };
}

/**
 * 把摘要渲染成一行文本。
 *
 * `已是最新` 只在它是唯一类别时显示：其余情况下它是默认背景，不值得占位。
 */
export function formatLibraryUpdateSummary(
  summary: LibraryUpdateSummary,
  t: (key: string, options?: Record<string, unknown>) => string,
): string {
  return formatLibraryUpdateSummaryItems(summary, t)
    .map((item) => item.text)
    .join(' · ');
}

export function formatLibraryUpdateSummaryItems(
  summary: LibraryUpdateSummary,
  t: (key: string, options?: Record<string, unknown>) => string,
): LibraryUpdateSummaryItem[] {
  // 尚未检查是空状态：旁边就摆着"检查更新"按钮，再写一遍不携带信息。
  if (summary.unchecked) {
    return summary.incomplete
      ? [{ text: t('libraries.updateSummary.incomplete'), tone: 'warning' }]
      : [];
  }

  const attention = ATTENTION_ORDER
    .filter((status) => summary[status] > 0)
    .map((status): LibraryUpdateSummaryItem => ({
      text: t(`libraries.updateSummary.${status}`, { count: summary[status] }),
      tone: status === 'updateAvailable' ? 'accent' : 'warning',
    }));

  const items: LibraryUpdateSummaryItem[] = [];
  if (attention.length === 0) {
    if (summary.upToDate > 0) {
      items.push({ text: t('libraries.updateSummary.allUpToDate'), tone: 'neutral' });
    }
  } else {
    items.push(...attention.slice(0, INLINE_CATEGORY_LIMIT));
    const remaining = attention.length - INLINE_CATEGORY_LIMIT;
    if (remaining > 0) {
      items.push({
        text: t('libraries.updateSummary.moreAttention', { count: remaining }),
        tone: 'warning',
      });
    }
  }
  if (summary.incomplete) {
    items.push({ text: t('libraries.updateSummary.incomplete'), tone: 'warning' });
  }
  return items;
}
