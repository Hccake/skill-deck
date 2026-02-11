// src/components/skills/SkillCard.tsx
import { useTranslation } from 'react-i18next';
import { cn, formatTime } from '@/lib/utils';
import {
  ArrowUpCircle,
  Trash2,
  ExternalLink,
  Globe,
  Folder,
  AlertTriangle,
} from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Card, CardContent } from '@/components/ui/card';
import { Toggle } from '@/components/ui/toggle';
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from '@/components/ui/tooltip';
import type { AgentType, Skill, SkillScope } from '@/types';

/** 默认空 Map，避免每次 render 创建新引用 — rerender-memo-with-default-value 规则 */
const EMPTY_DISPLAY_NAMES = new Map<AgentType, string>();

interface SkillCardProps {
  skill: Skill;
  /** 当前显示的 scope（用于决定图标） */
  displayScope: SkillScope;
  /** 是否存在冲突（同时在 project 和 global 安装） */
  hasConflict?: boolean;
  /** 是否正在更新中 */
  isUpdating?: boolean;
  /** Agent display name 映射（agentId → displayName） */
  agentDisplayNames?: Map<AgentType, string>;
  /** 点击卡片打开详情 */
  onClick?: (skill: Skill) => void;
  onUpdate?: (skillName: string) => void;
  onDelete?: (skillName: string) => void;
  onToggleAgent?: (skillName: string, agentId: string) => void;
}

export function SkillCard({
  skill,
  displayScope,
  hasConflict = false,
  isUpdating = false,
  agentDisplayNames = EMPTY_DISPLAY_NAMES,
  onClick,
  onUpdate,
  onDelete,
  onToggleAgent,
}: SkillCardProps) {
  const { t, i18n } = useTranslation();

  const ScopeIcon = displayScope === 'global' ? Globe : Folder;
  const scopeTooltip = t(`skills.scopeIcon.${displayScope}`);
  const conflictTooltip =
    displayScope === 'project'
      ? t('skills.conflict.alsoInGlobal')
      : t('skills.conflict.alsoInProject');

  return (
    <TooltipProvider>
      <Card
        className={cn(
          "py-0 gap-0 cursor-pointer transition-colors hover:bg-accent/50",
          skill.hasUpdate && "border-l-2 border-l-warning"
        )}
        onClick={() => onClick?.(skill)}
      >
        <CardContent className="p-3 sm:p-4">
          {/* Row 1: Scope Icon + Name + Conflict Icon + Actions */}
          <div className="flex items-center justify-between mb-2">
            <div className="flex items-center gap-2 sm:gap-2.5">
              {/* Scope Icon */}
              <Tooltip>
                <TooltipTrigger asChild>
                  <div className="flex h-8 w-8 items-center justify-center rounded-lg bg-accent">
                    <ScopeIcon className="h-4 w-4 text-accent-foreground" />
                  </div>
                </TooltipTrigger>
                <TooltipContent>
                  <p>{scopeTooltip}</p>
                </TooltipContent>
              </Tooltip>

              {/* Skill Name */}
              <h3 className="text-sm font-semibold text-foreground">{skill.name}</h3>

              {/* Conflict Icon */}
              {hasConflict && (
                <Tooltip>
                  <TooltipTrigger asChild>
                    <AlertTriangle className="h-4 w-4 text-warning" />
                  </TooltipTrigger>
                  <TooltipContent>
                    <p>{conflictTooltip}</p>
                  </TooltipContent>
                </Tooltip>
              )}
            </div>

            {/* Action buttons */}
            <div className="flex items-center gap-0.5 sm:gap-1">
              {skill.hasUpdate && (
                <Button
                  variant="ghost"
                  size="icon"
                  className="h-7 w-7 text-warning hover:text-warning hover:bg-warning/10 cursor-pointer"
                  aria-label={t('skills.actions.update')}
                  title={t('skills.actions.update')}
                  disabled={isUpdating}
                  onClick={(e) => {
                    e.stopPropagation();
                    onUpdate?.(skill.name);
                  }}
                >
                  <ArrowUpCircle className={`h-3.5 w-3.5 ${isUpdating ? 'animate-spin' : ''}`} />
                </Button>
              )}
              <Button
                variant="ghost"
                size="icon"
                className="h-7 w-7 text-muted-foreground hover:text-destructive hover:bg-destructive/10 cursor-pointer"
                aria-label={t('skills.actions.delete')}
                title={t('skills.actions.delete')}
                onClick={(e) => {
                  e.stopPropagation();
                  onDelete?.(skill.name);
                }}
              >
                <Trash2 className="h-3.5 w-3.5" />
              </Button>
            </div>
          </div>

          {/* Row 2: Description */}
          <p className="text-sm text-muted-foreground mb-2 leading-relaxed">
            {skill.description}
          </p>

          {/* Row 3: Source + Updated */}
          <div className="flex items-center gap-2 text-xs text-muted-foreground mb-3">
            {skill.sourceUrl && skill.source && (
              <>
                <a
                  href={skill.sourceUrl}
                  target="_blank"
                  rel="noopener noreferrer"
                  className="flex items-center gap-1 hover:text-foreground transition-colors cursor-pointer"
                  onClick={(e) => e.stopPropagation()}
                >
                  <span className="truncate max-w-[120px] sm:max-w-none">{skill.source}</span>
                  <ExternalLink className="h-3 w-3 flex-shrink-0" />
                </a>
                <span className="text-border">·</span>
              </>
            )}
            {skill.updatedAt && (
              <span>{t('skills.updated', { time: formatTime(skill.updatedAt, i18n.language) })}</span>
            )}
          </div>

          {/* Row 4: Agents */}
          <div className="flex items-center gap-1 sm:gap-1.5 flex-wrap">
            {skill.agents.map((agentId) => (
              <Toggle
                key={agentId}
                pressed={true}
                onPressedChange={() => onToggleAgent?.(skill.name, agentId)}
                size="sm"
                variant="outline"
                className="h-7 px-2 sm:px-2.5 text-xs gap-1 sm:gap-1.5"
                aria-label={t('skills.agent.disable', { name: agentDisplayNames.get(agentId) ?? agentId })}
                onClick={(e) => e.stopPropagation()}
              >
                <span className="flex h-1.5 w-1.5 rounded-full bg-success" />
                {agentDisplayNames.get(agentId) ?? agentId}
              </Toggle>
            ))}
          </div>
        </CardContent>
      </Card>
    </TooltipProvider>
  );
}
