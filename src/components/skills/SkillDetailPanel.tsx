// src/components/skills/SkillDetailPanel.tsx
import { memo, useState, useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { Globe, Folder, ExternalLink, Copy, Check, X, RefreshCw, Trash2, ArrowUpCircle } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { ScrollArea } from '@/components/ui/scroll-area';
import { Skeleton } from '@/components/ui/skeleton';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { formatTime } from '@/lib/utils';
import type { InstalledSkill, SkillScope } from '@/bindings';

interface SkillDetailPanelProps {
  skill: InstalledSkill;
  content: string | null;
  loading: boolean;
  agentDisplayNames: Map<string, string>;
  onClose: () => void;
  onUpdate: (name: string, scope: SkillScope) => void;
  onDelete: (skill: InstalledSkill) => void;
  onRetry: () => void;
}

export const SkillDetailPanel = memo(function SkillDetailPanel({
  skill,
  content,
  loading,
  agentDisplayNames,
  onClose,
  onUpdate,
  onDelete,
  onRetry,
}: SkillDetailPanelProps) {
  const { t, i18n } = useTranslation();
  const [copied, setCopied] = useState(false);

  const ScopeIcon = skill.scope === 'global' ? Globe : Folder;

  const handleCopyPath = useCallback(async () => {
    try {
      await navigator.clipboard.writeText(skill.canonicalPath);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch {
      console.error('Failed to copy path');
    }
  }, [skill.canonicalPath]);

  const handleUpdate = useCallback(() => {
    onUpdate(skill.name, skill.scope);
  }, [onUpdate, skill.name, skill.scope]);

  const handleDelete = useCallback(() => {
    onDelete(skill);
  }, [onDelete, skill]);

  return (
    <div className="h-full flex flex-col overflow-hidden bg-background">
      <div className="flex items-center justify-between px-4 sm:px-5 py-2.5 border-b border-border/80 flex-shrink-0 bg-background/95 backdrop-blur z-10">
        <div className="flex items-center gap-2 min-w-0 text-muted-foreground">
          <ScopeIcon className="h-3.5 w-3.5 shrink-0" />
          <span className="text-[13px] font-medium truncate tracking-tight">{skill.name}</span>
        </div>
        <div className="flex items-center gap-0.5 shrink-0">
          {skill.hasUpdate ? (
            <Button
              variant="ghost"
              size="icon-xs"
              className="h-6 w-6 text-warning hover:text-warning hover:bg-warning/10 cursor-pointer"
              title={t('skills.actions.update')}
              onClick={handleUpdate}
            >
              <ArrowUpCircle className="h-3.5 w-3.5" />
            </Button>
          ) : null}
          <Button
            variant="ghost"
            size="icon-xs"
            className="h-6 w-6 text-muted-foreground hover:text-destructive hover:bg-destructive/10 cursor-pointer"
            title={t('skills.actions.delete')}
            onClick={handleDelete}
          >
            <Trash2 className="h-3.5 w-3.5" />
          </Button>
          <Button variant="ghost" size="icon-xs" className="h-6 w-6 cursor-pointer text-muted-foreground" onClick={onClose}>
            <X className="h-3.5 w-3.5" />
          </Button>
        </div>
      </div>

      {/* 沉浸式滚动文档流 (Scrollable Document Area) */}
      <div className="flex-1 min-h-0 relative">
        <ScrollArea className="absolute inset-0 w-full h-full">
          <div className="px-4 py-4 sm:px-6 sm:py-5 w-full space-y-4">
            
            {/* 系统属性底色条 (Meta Properties Subheader) */}
            <div className="flex flex-col gap-2 pb-3 border-b border-border/40">
              {/* 源信息与时间 */}
              <div className="flex flex-wrap items-center gap-2 text-[11px] text-muted-foreground">
                {skill.source && skill.sourceUrl ? (
                  <a
                    href={skill.sourceUrl}
                    target="_blank"
                    rel="noopener noreferrer"
                    className="inline-flex items-center gap-1 text-primary/80 hover:text-primary transition-colors hover:underline font-medium"
                  >
                    {skill.source}
                    <ExternalLink className="h-3 w-3" />
                  </a>
                ) : null}
                
                {skill.gitRef ? (
                  <>
                    <span className="text-border/60">·</span>
                    <span className="font-mono bg-muted/50 px-1 py-0.5 rounded text-[10px]">
                      {skill.gitRef}
                    </span>
                  </>
                ) : null}

                {(skill.installedAt || skill.updatedAt) ? (
                  <span className="hidden sm:inline text-border/60">·</span>
                ) : null}
                <div className="hidden sm:flex items-center gap-2.5">
                  {skill.installedAt ? (
                    <span>{t('skills.detail.installed')}: {formatTime(skill.installedAt, i18n.language)}</span>
                  ) : null}
                  {skill.updatedAt ? (
                    <span>{t('skills.detail.updated')}: {formatTime(skill.updatedAt, i18n.language)}</span>
                  ) : null}
                </div>
              </div>

              {/* Agents 标签与本地路径 */}
              <div className="flex flex-wrap items-center justify-between gap-3">
                <div className="flex flex-wrap gap-1">
                  {skill.agents.length > 0 ? (
                    skill.agents.map((agentId) => (
                      <span
                        key={agentId}
                        className="inline-flex items-center gap-1 rounded bg-muted/40 px-1.5 py-0.5 text-[11px] font-medium text-foreground/80 border border-border/40"
                      >
                        <span className="flex h-1 w-1 rounded-full bg-success opacity-80" />
                        {agentDisplayNames.get(agentId) ?? agentId}
                      </span>
                    ))
                  ) : (
                    <span className="text-[11px] text-muted-foreground/60">{t('skills.detail.noAgents')}</span>
                  )}
                </div>

                <div className="flex items-center gap-1 flex-shrink-0">
                  <code className="text-[10px] text-muted-foreground/80 font-mono bg-muted/30 px-1.5 py-0.5 rounded max-w-[200px] truncate border border-border/30">
                    {skill.canonicalPath}
                  </code>
                  <Tooltip>
                    <TooltipTrigger asChild>
                      <Button variant="ghost" size="icon-xs" className="h-5 w-5 text-muted-foreground hover:bg-muted/50 border-none shadow-none" onClick={handleCopyPath}>
                        {copied ? (
                          <Check className="h-3 w-3 text-success" />
                        ) : (
                          <Copy className="h-3 w-3" />
                        )}
                      </Button>
                    </TooltipTrigger>
                    <TooltipContent><p>{t('common.copy')}</p></TooltipContent>
                  </Tooltip>
                </div>
              </div>
            </div>

            {/* Markdown 正文 */}
            <div className="pb-10">
              {loading ? (
                <div className="space-y-4">
                  <Skeleton className="h-6 w-1/3" />
                  <Skeleton className="h-4 w-full" />
                  <Skeleton className="h-4 w-5/6" />
                  <Skeleton className="h-4 w-11/12" />
                  <Skeleton className="h-32 w-full mt-6" />
                </div>
              ) : content ? (
                <div className="skill-prose">
                  <MarkdownContent content={content} />
                </div>
              ) : (
                <div className="flex flex-col items-center justify-center py-20 text-muted-foreground">
                  <p className="text-sm pb-4">{t('skills.detail.emptyContent')}</p>
                  <Button variant="outline" size="sm" onClick={onRetry} className="bg-transparent">
                    <RefreshCw className="h-3.5 w-3.5 mr-2" />
                    {t('skills.detail.retry')}
                  </Button>
                </div>
              )}
            </div>

          </div>
        </ScrollArea>
      </div>
    </div>
  );
});

// Extracted for memo optimization (rerender-memo)
const MarkdownContent = memo(function MarkdownContent({ content }: { content: string }) {
  return <ReactMarkdown remarkPlugins={[remarkGfm]}>{content}</ReactMarkdown>;
});
