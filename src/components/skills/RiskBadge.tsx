// src/components/skills/RiskBadge.tsx
import { memo } from 'react';
import { useTranslation } from 'react-i18next';
import { Badge } from '@/components/ui/badge';
import { cn } from '@/lib/utils';
import type { RiskLevel } from '@/bindings';

/** 风险等级 → 样式映射 */
const RISK_STYLES: Record<RiskLevel, string> = {
  safe: 'bg-emerald-500/15 text-emerald-700 dark:text-emerald-400 border-emerald-500/25',
  low: 'bg-blue-500/15 text-blue-700 dark:text-blue-400 border-blue-500/25',
  medium: 'bg-yellow-500/15 text-yellow-700 dark:text-yellow-400 border-yellow-500/25',
  high: 'bg-orange-500/15 text-orange-700 dark:text-orange-400 border-orange-500/25',
  critical: 'bg-red-500/15 text-red-700 dark:text-red-400 border-red-500/25',
  unknown: 'bg-muted text-muted-foreground border-border',
};

interface RiskBadgeProps {
  risk: RiskLevel;
  className?: string;
}

export const RiskBadge = memo(function RiskBadge({ risk, className }: RiskBadgeProps) {
  const { t } = useTranslation();

  return (
    <Badge
      variant="outline"
      className={cn('text-[10px] px-1.5 py-0', RISK_STYLES[risk], className)}
    >
      {t(`audit.risk.${risk}`)}
    </Badge>
  );
});
