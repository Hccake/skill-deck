import { Loader2, X } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { AgentDefinitionForm } from './AgentDefinitionForm';
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert';
import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import type { AgentFieldError, CustomAgentDefinition } from '@/bindings';

export type AgentDefinitionDialogMode = 'create' | 'edit' | 'duplicate' | 'configure';

interface AgentDefinitionDialogProps {
  draft: CustomAgentDefinition | null;
  mode: AgentDefinitionDialogMode;
  originalId: string | null;
  errors: AgentFieldError[];
  readOnly: boolean;
  saving: boolean;
  stale: boolean;
  deleted: boolean;
  configurationPersisted: boolean;
  onChange: (draft: CustomAgentDefinition) => void;
  onRequestClose: () => void;
  onSave: () => void;
  onReload: () => void;
}

export function AgentDefinitionDialog({
  draft,
  mode,
  originalId,
  errors,
  readOnly,
  saving,
  stale,
  deleted,
  configurationPersisted,
  onChange,
  onRequestClose,
  onSave,
  onReload,
}: AgentDefinitionDialogProps) {
  const { t } = useTranslation();

  return (
    <Dialog
      open={draft !== null}
      onOpenChange={(open) => {
        if (!open && !saving) onRequestClose();
      }}
    >
      <DialogContent
        showCloseButton={false}
        dismissible={!saving}
        className="h-[min(52rem,calc(100vh-2rem))] min-w-0 grid-rows-[auto_minmax(0,1fr)_auto] gap-0 overflow-hidden p-0 sm:max-w-3xl"
        aria-busy={saving}
        onOpenAutoFocus={(event) => {
          event.preventDefault();
          document.getElementById('agent-name')?.focus();
        }}
      >
        <DialogHeader className="relative border-b border-border/60 px-5 py-4 pr-14 text-left">
          <DialogTitle className="text-base">
            {t(`settings.agents.dialog.title.${mode}`)}
          </DialogTitle>
          <DialogDescription className="text-xs leading-5">
            {t(`settings.agents.dialog.description.${mode === 'configure' ? 'configure' : 'default'}`)}
          </DialogDescription>
          <Button
            type="button"
            variant="ghost"
            size="icon-sm"
            className="absolute right-4 top-4 text-muted-foreground"
            disabled={saving}
            aria-label={t('common.close')}
            onClick={onRequestClose}
          >
            <X className="h-4 w-4" />
          </Button>
        </DialogHeader>

        <div className="min-h-0 overflow-y-auto overscroll-contain px-5 py-4">
          {configurationPersisted ? (
            <Alert className="mb-4">
              <AlertTitle>{t('settings.agents.configurationPending.title')}</AlertTitle>
              <AlertDescription>{t('settings.agents.configurationPending.description')}</AlertDescription>
            </Alert>
          ) : null}
          {draft ? (
            <AgentDefinitionForm
              draft={draft}
              originalId={originalId}
              idReadOnly={mode === 'configure'}
              errors={errors}
              disabled={readOnly || saving || configurationPersisted || deleted}
              stale={stale}
              deleted={deleted}
              onChange={onChange}
              onReload={onReload}
            />
          ) : null}
        </div>

        <DialogFooter className="border-t border-border/60 bg-muted/10 px-5 py-3">
          {!configurationPersisted ? (
            <Button type="button" variant="outline" disabled={saving} onClick={onRequestClose}>
              {mode === 'configure'
                ? t('settings.agents.dialog.cancel.configure')
                : t('common.cancel')}
            </Button>
          ) : null}
          <Button
            type="button"
            disabled={readOnly || saving || stale || deleted}
            onClick={onSave}
          >
            {saving ? <Loader2 className="h-3.5 w-3.5 animate-spin" /> : null}
            {t(configurationPersisted
              ? 'settings.agents.dialog.action.completeConfiguration'
              : `settings.agents.dialog.action.${mode}`)}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
