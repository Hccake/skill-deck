import { AlertTriangle } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Checkbox } from '@/components/ui/checkbox';

interface RedirectHostConfirmationProps {
  host: string;
  acknowledged: boolean;
  onAcknowledgedChange: (acknowledged: boolean) => void;
}

export function RedirectHostConfirmation({
  host,
  acknowledged,
  onAcknowledgedChange,
}: RedirectHostConfirmationProps) {
  const { t } = useTranslation();

  return (
    <div className="space-y-2 rounded-md border border-warning/40 bg-warning/10 px-3 py-3">
      <div className="flex items-start gap-2">
        <AlertTriangle className="mt-0.5 size-4 shrink-0 text-warning" aria-hidden="true" />
        <div className="min-w-0 space-y-1">
          <p className="text-sm font-medium">{t('addSkill.confirm.redirectTitle')}</p>
          <p className="break-words text-sm text-muted-foreground [overflow-wrap:anywhere]">
            {t('addSkill.confirm.redirectBody', { host })}
          </p>
        </div>
      </div>
      <label className="flex cursor-pointer items-start gap-2 text-sm">
        <Checkbox
          checked={acknowledged}
          onCheckedChange={(checked) => onAcknowledgedChange(checked === true)}
          aria-label={t('addSkill.confirm.redirectAcknowledge')}
          className="mt-0.5"
        />
        <span>{t('addSkill.confirm.redirectAcknowledge')}</span>
      </label>
    </div>
  );
}
