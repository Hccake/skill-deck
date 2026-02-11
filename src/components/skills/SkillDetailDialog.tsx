// src/components/skills/SkillDetailDialog.tsx
import { useTranslation } from 'react-i18next';
import { Globe, Folder, ExternalLink, Copy, Check } from 'lucide-react';
import { useState } from 'react';
import {
  Dialog,
  DialogContent,
  DialogTitle,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from '@/components/ui/tooltip';
import type { Skill } from '@/types';

interface SkillDetailDialogProps {
  skill: Skill | null;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

export function SkillDetailDialog({
  skill,
  open,
  onOpenChange,
}: SkillDetailDialogProps) {
  const { t } = useTranslation();
  const [copied, setCopied] = useState(false);

  if (!skill) return null;

  const ScopeIcon = skill.scope === 'global' ? Globe : Folder;
  const scopeLabel = t(`skills.scopeIcon.${skill.scope}`);

  const handleCopyPath = async () => {
    try {
      await navigator.clipboard.writeText(skill.canonicalPath);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch {
      console.error('Failed to copy path');
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-xl">
        {/* 隐藏的标题用于无障碍访问 */}
        <DialogTitle className="sr-only">{t('skills.detail.title')}</DialogTitle>

        <div className="space-y-5">
          {/* Header: Icon + Name + Scope Badge (与关闭按钮同行) */}
          <div className="flex items-start gap-3 pr-6">
            <div className="flex h-10 w-10 shrink-0 items-center justify-center rounded-lg bg-primary/10">
              <ScopeIcon className="h-5 w-5 text-primary" />
            </div>
            <div className="min-w-0 flex-1">
              <h3 className="text-base font-semibold leading-tight">{skill.name}</h3>
              <span className="mt-0.5 inline-flex items-center rounded-full bg-muted px-2 py-0.5 text-xs text-muted-foreground">
                {scopeLabel}
              </span>
            </div>
          </div>

          {/* Description */}
          <p className="text-sm leading-relaxed text-muted-foreground">
            {skill.description}
          </p>

          {/* Info Grid: Source + Timestamps */}
          <div className="grid grid-cols-2 gap-x-4 gap-y-3 text-sm">
            {/* Source */}
            <div className="col-span-2">
              <dt className="text-xs font-medium uppercase tracking-wide text-muted-foreground/70">
                {t('skills.detail.source')}
              </dt>
              <dd className="mt-1">
                {skill.source && skill.sourceUrl ? (
                  <a
                    href={skill.sourceUrl}
                    target="_blank"
                    rel="noopener noreferrer"
                    className="inline-flex items-center gap-1.5 text-primary hover:underline"
                  >
                    {skill.source}
                    <ExternalLink className="h-3.5 w-3.5" />
                  </a>
                ) : (
                  <span className="text-muted-foreground">
                    {t('skills.detail.noSource')}
                  </span>
                )}
              </dd>
            </div>

            {/* Installed */}
            {skill.installedAt && (
              <div>
                <dt className="text-xs font-medium uppercase tracking-wide text-muted-foreground/70">
                  {t('skills.detail.installed')}
                </dt>
                <dd className="mt-1">{skill.installedAt}</dd>
              </div>
            )}

            {/* Updated */}
            {skill.updatedAt && (
              <div>
                <dt className="text-xs font-medium uppercase tracking-wide text-muted-foreground/70">
                  {t('skills.detail.updated')}
                </dt>
                <dd className="mt-1">{skill.updatedAt}</dd>
              </div>
            )}
          </div>

          {/* Path - Full width with word break */}
          <div>
            <div className="flex items-center justify-between gap-2">
              <dt className="text-xs font-medium uppercase tracking-wide text-muted-foreground/70">
                {t('skills.detail.path')}
              </dt>
              <TooltipProvider>
                <Tooltip>
                  <TooltipTrigger asChild>
                    <Button
                      variant="ghost"
                      size="icon"
                      className="h-7 w-7 shrink-0 cursor-pointer"
                      onClick={handleCopyPath}
                    >
                      {copied ? (
                        <Check className="h-3.5 w-3.5 text-success" />
                      ) : (
                        <Copy className="h-3.5 w-3.5" />
                      )}
                    </Button>
                  </TooltipTrigger>
                  <TooltipContent>
                    <p>{t('common.copy')}</p>
                  </TooltipContent>
                </Tooltip>
              </TooltipProvider>
            </div>
            <div className="mt-1.5 rounded-md bg-muted p-2">
              <code className="text-xs font-mono break-all">
                {skill.canonicalPath}
              </code>
            </div>
          </div>

          {/* Synced Agents */}
          <div>
            <dt className="text-xs font-medium uppercase tracking-wide text-muted-foreground/70">
              {t('skills.detail.syncedAgents')}
            </dt>
            <dd className="mt-2">
              {skill.agents.length > 0 ? (
                <div className="flex flex-wrap gap-2">
                  {skill.agents.map((agentId) => (
                    <span
                      key={agentId}
                      className="inline-flex items-center gap-1.5 rounded-full border bg-background px-2.5 py-1 text-xs font-medium"
                    >
                      <span className="h-2 w-2 rounded-full bg-success" />
                      {agentId}
                    </span>
                  ))}
                </div>
              ) : (
                <p className="text-sm text-muted-foreground">
                  {t('skills.detail.noAgents')}
                </p>
              )}
            </dd>
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );
}
