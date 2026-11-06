import { useId } from 'react';
import { Trans, useTranslation } from 'react-i18next';
import { Checkbox } from '@/components/ui/checkbox';

interface RedirectHostConfirmationProps {
  sourceUrl: string;
  host: string;
  action?: 'install' | 'add';
  acknowledged: boolean;
  onAcknowledgedChange: (acknowledged: boolean) => void;
}

export function RedirectHostConfirmation({ sourceUrl, host, action = 'install', acknowledged, onAcknowledgedChange }: RedirectHostConfirmationProps) {
  const { t } = useTranslation();
  const descriptionId = useId();
  let original: string | null = null;
  try { original = new URL(sourceUrl).hostname; } catch { /* 旧发现结果缺少原始 URL 时仍展示实际下载站点。 */ }
  const label = t(action === 'add' ? 'addSkill.confirm.redirectAllowAdd' : 'addSkill.confirm.redirectAcknowledge');

  return (
    <fieldset className="m-0 grid min-w-0 grid-cols-[16px_minmax(0,1fr)] gap-x-[9px] gap-y-1 border-0 border-t border-border pt-3 text-[13px] leading-5">
      <legend className="sr-only">{t('addSkill.confirm.redirectTitle')}</legend>
      <label className="col-span-2 flex cursor-pointer items-start gap-[9px] text-foreground">
        <Checkbox checked={acknowledged} onCheckedChange={checked => onAcknowledgedChange(checked === true)} aria-label={label} aria-describedby={descriptionId} className="mt-0.5 shrink-0" />
        <span>{label}</span>
      </label>
      <p id={descriptionId} className="col-start-2 min-w-0 text-muted-foreground [overflow-wrap:anywhere]">
        <Trans i18nKey={original ? 'addSkill.confirm.redirectBody' : 'addSkill.confirm.redirectDestination'} values={{ original, host }} components={{ host: <strong className="font-medium text-foreground" /> }} />
      </p>
    </fieldset>
  );
}
