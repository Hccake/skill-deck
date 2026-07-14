import type { ActiveMutation } from '@/bindings';

type Translate = (key: string, options?: Record<string, unknown>) => string;

export function formatMutationStatus(mutation: ActiveMutation, t: Translate): string {
  const values = {
    operation: t(`mutation.kind.${mutation.kind}`),
    phase: t(`mutation.phase.${mutation.phase}`),
    subject: mutation.progress?.subject ?? '',
  };
  return mutation.progress?.subject
    ? t('mutation.activityWithSubject', values)
    : t('mutation.activity', values);
}
