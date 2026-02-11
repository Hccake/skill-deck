// src/components/skills/EmptyStates.tsx
import { useTranslation } from 'react-i18next';
import { Package, Sparkles } from 'lucide-react';
import { Button } from '@/components/ui/button';

interface GlobalEmptyStateProps {
  onAdd: () => void;
}

export function GlobalEmptyState({ onAdd }: GlobalEmptyStateProps) {
  const { t } = useTranslation();

  return (
    <div className="flex flex-col items-center justify-center min-h-48 py-8">
      <div className="relative">
        <div className="absolute inset-0 bg-gradient-to-r from-primary/10 to-blue-500/10 blur-2xl rounded-full scale-150" />
        <div className="relative flex flex-col items-center">
          <div className="relative mb-4 sm:mb-5">
            <div className="absolute inset-0 bg-gradient-to-br from-blue-500 to-indigo-600 rounded-2xl blur-lg opacity-30" />
            <div className="relative flex h-14 w-14 sm:h-16 sm:w-16 items-center justify-center rounded-2xl bg-gradient-to-br from-blue-500 to-indigo-600 shadow-xl">
              <Package className="h-7 w-7 sm:h-8 sm:w-8 text-white" />
            </div>
            <div className="absolute -top-1 -right-1 flex h-5 w-5 items-center justify-center rounded-full bg-amber-400 shadow-md">
              <Sparkles className="h-3 w-3 text-amber-900" />
            </div>
          </div>

          <h2 className="text-sm sm:text-base font-semibold text-foreground mb-1.5">
            {t('skills.empty')}
          </h2>
          <p className="text-xs sm:text-sm text-muted-foreground text-center max-w-[240px] sm:max-w-[260px] mb-4 sm:mb-5">
            {t('skills.emptyHint')}
          </p>

          <Button size="default" className="gap-2 shadow-md cursor-pointer" onClick={onAdd}>
            <Package className="h-4 w-4" />
            {t('skills.add')}
          </Button>
        </div>
      </div>
    </div>
  );
}

export function ProjectEmptyState() {
  const { t } = useTranslation();

  return (
    <div className="relative overflow-hidden rounded-xl border border-dashed border-border/80 bg-accent/20 p-5">
      <div className="flex flex-col items-center text-center">
        <div className="flex h-10 w-10 items-center justify-center rounded-full bg-muted mb-2.5">
          <Package className="h-5 w-5 text-muted-foreground" />
        </div>
        <p className="text-sm font-medium text-foreground mb-1">{t('skills.empty')}</p>
        <p className="text-xs text-muted-foreground max-w-[220px]">
          {t('skills.emptyHint')}
        </p>
      </div>
    </div>
  );
}
