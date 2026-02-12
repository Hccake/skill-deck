import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { XCircle, ChevronDown, ChevronUp, Lightbulb, RotateCw } from 'lucide-react';
import { Button } from '@/components/ui/button';
import type { InstallError } from './types';

interface ErrorStepProps {
  error: InstallError;
  onRetry: () => void;
  onBack: () => void;
  onClose: () => void;
}

export function ErrorStep({ error, onRetry, onBack, onClose }: ErrorStepProps) {
  const { t } = useTranslation();
  const [detailsExpanded, setDetailsExpanded] = useState(true);

  return (
    <div className="space-y-4 py-4">
      {/* Header */}
      <div className="flex items-center gap-3">
        <XCircle className="h-8 w-8 text-destructive shrink-0" />
        <div>
          <h3 className="text-lg font-medium">{t('addSkill.error.title')}</h3>
          <p className="text-sm text-muted-foreground mt-1">{error.message}</p>
        </div>
      </div>

      {/* Details (collapsible) */}
      {error.details && (
        <div className="border rounded-md overflow-hidden">
          <button
            onClick={() => setDetailsExpanded(!detailsExpanded)}
            className="w-full flex items-center justify-between p-3 bg-muted/30 hover:bg-muted/50 transition-colors text-left"
          >
            <span className="text-sm font-medium">{t('addSkill.error.detailsLabel')}</span>
            {detailsExpanded ? (
              <ChevronUp className="h-4 w-4" />
            ) : (
              <ChevronDown className="h-4 w-4" />
            )}
          </button>
          {detailsExpanded && (
            <div className="p-3 text-sm text-muted-foreground whitespace-pre-wrap bg-muted/10">
              {error.details}
            </div>
          )}
        </div>
      )}

      {/* Suggestions */}
      {error.suggestions && error.suggestions.length > 0 && (
        <div className="border rounded-md p-4 bg-blue-50/50 dark:bg-blue-950/20">
          <div className="flex items-center gap-2 mb-3">
            <Lightbulb className="h-4 w-4 text-blue-600 dark:text-blue-400" />
            <span className="text-sm font-medium text-blue-900 dark:text-blue-100">
              {t('addSkill.error.suggestionsLabel')}
            </span>
          </div>
          <ul className="space-y-2 text-sm text-blue-800 dark:text-blue-200">
            {error.suggestions.map((suggestion, index) => (
              <li key={index} className="flex items-start gap-2">
                <span className="text-blue-600 dark:text-blue-400 shrink-0">•</span>
                <span>{suggestion}</span>
              </li>
            ))}
          </ul>
        </div>
      )}

      {/* Actions */}
      <div className="flex justify-end gap-2 pt-2">
        <Button variant="outline" onClick={onClose}>
          {t('addSkill.error.actions.close')}
        </Button>
        <Button variant="outline" onClick={onBack}>
          {t('addSkill.error.actions.backToSource')}
        </Button>
        <Button onClick={onRetry}>
          <RotateCw className="h-4 w-4 mr-1.5" />
          {t('addSkill.error.actions.retry')}
        </Button>
      </div>
    </div>
  );
}
