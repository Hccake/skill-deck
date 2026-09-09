import { useTranslation } from 'react-i18next';
import type { LibraryMembershipPreview, MembershipScopeImpactKind } from '@/bindings';

const IMPACT_ORDER: MembershipScopeImpactKind[] = [
  'added',
  'switched',
  'fallback',
  'removed',
  'unchanged',
  'unverified',
];

export function MembershipImpactSummary({ preview }: { preview: LibraryMembershipPreview }) {
  const { t } = useTranslation();
  const counts = new Map<MembershipScopeImpactKind, number>();
  for (const impact of preview.impacts) {
    for (const skill of impact.skills) {
      counts.set(skill.kind, (counts.get(skill.kind) ?? 0) + 1);
    }
  }

  return (
    <div className="flex flex-wrap gap-x-3 gap-y-1 text-xs text-muted-foreground" role="status">
      {IMPACT_ORDER.map((kind) => {
        const count = counts.get(kind) ?? 0;
        return count > 0 ? (
          <span key={kind}>{t(`libraries.membership.impact.${kind}`, { count })}</span>
        ) : null;
      })}
    </div>
  );
}
