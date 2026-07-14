// src/components/skills/CopyToProjectDialog.tsx
import { useState, useCallback, useMemo, useEffect, memo } from 'react';
import { useTranslation } from 'react-i18next';
import { AlertTriangle, Folder, Info, Loader2 } from 'lucide-react';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Checkbox } from '@/components/ui/checkbox';
import { Label } from '@/components/ui/label';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import type { InstalledSkill } from '@/bindings';
import { useMutationStore } from '@/stores/mutation';

interface CopyToProjectDialogProps {
  skill: InstalledSkill | null;
  /** 当前项目路径（排除在目标列表中） */
  currentProjectPath: string;
  /** 所有已注册项目列表 */
  projects: string[];
  /** 检查 skill 在目标项目中是否已存在 */
  checkExistence?: (skillName: string, projectPaths: string[]) => Promise<Array<{ projectPath: string; hasSkill: boolean }>>;
  onClose: () => void;
  onCopy: (targetPaths: string[]) => Promise<void>;
}

const SOURCE_INFO_LIMIT_REASONS = new Set(['missing-skill-path', 'missing-remote-hash']);

export const CopyToProjectDialog = memo(function CopyToProjectDialog({
  skill,
  currentProjectPath,
  projects,
  checkExistence,
  onClose,
  onCopy,
}: CopyToProjectDialogProps) {
  const { t } = useTranslation();
  const writeBlocked = useMutationStore((state) => state.activeMutation !== null);
  const [copying, setCopying] = useState(false);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  /** 已安装此 skill 的项目路径集合 */
  const [existingSet, setExistingSet] = useState<Set<string>>(new Set());

  // render-time reset
  const [prevSkill, setPrevSkill] = useState(skill);
  if (skill !== prevSkill) {
    setPrevSkill(skill);
    setSelected(new Set());
    setExistingSet(new Set());
  }

  // 过滤掉当前项目
  const availableProjects = useMemo(
    () => (projects ?? []).filter((p) => p !== currentProjectPath),
    [projects, currentProjectPath],
  );

  // skill 打开时检查已存在状态
  useEffect(() => {
    if (!skill || !checkExistence || availableProjects.length === 0) return;
    let cancelled = false;
    checkExistence(skill.name, availableProjects).then((statuses) => {
      if (cancelled) return;
      const existing = new Set(statuses.filter((s) => s.hasSkill).map((s) => s.projectPath));
      setExistingSet(existing);
    }).catch(() => { /* 静默失败，不影响正常使用 */ });
    return () => { cancelled = true; };
  }, [skill, checkExistence, availableProjects]);

  // 选中的项目中有多少个已存在此 skill
  const selectedExistingCount = useMemo(() => {
    let count = 0;
    for (const path of selected) {
      if (existingSet.has(path)) count++;
    }
    return count;
  }, [selected, existingSet]);

  const showSourceInfoNote = useMemo(() => {
    if (!skill) return false;
    if (!skill.source && !skill.sourceUrl) return true;
    return SOURCE_INFO_LIMIT_REASONS.has(skill.updateReason ?? '');
  }, [skill]);

  const toggleProject = useCallback((path: string) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(path)) {
        next.delete(path);
      } else {
        next.add(path);
      }
      return next;
    });
  }, []);

  const handleCopy = useCallback(async () => {
    setCopying(true);
    try {
      await onCopy(Array.from(selected));
    } finally {
      setCopying(false);
    }
  }, [onCopy, selected]);

  return (
    <Dialog open={!!skill} onOpenChange={(open) => !open && !copying && onClose()}>
      <DialogContent className="sm:max-w-md gap-0">
        <DialogHeader>
          <DialogTitle>{t('skills.copyToProject.title')}</DialogTitle>
          <DialogDescription>
            {t('skills.copyToProject.description', { name: skill?.name })}
          </DialogDescription>
        </DialogHeader>

        {showSourceInfoNote ? (
          <div role="note" className="mt-4 flex items-start gap-1.5 rounded-md bg-muted/40 px-2.5 py-2">
            <Info className="h-3.5 w-3.5 shrink-0 mt-px text-muted-foreground" />
            <p className="text-xs text-muted-foreground leading-relaxed">
              {t('skills.copyToProject.metadataWarning')}
            </p>
          </div>
        ) : null}

        <div className={showSourceInfoNote ? 'mt-2 space-y-1.5 max-h-[50vh] overflow-y-auto' : 'mt-4 space-y-1.5 max-h-[50vh] overflow-y-auto'}>
          {availableProjects.length > 0 ? (
            <>
              {availableProjects.map((path) => {
                const hasSkill = existingSet.has(path);
                return (
                  <div
                    key={path}
                    className="flex items-center gap-3 p-2 rounded-md hover:bg-muted/50 cursor-pointer"
                    onClick={() => toggleProject(path)}
                  >
                    <Checkbox checked={selected.has(path)} />
                    <Folder className="h-4 w-4 text-muted-foreground shrink-0" />
                    <Label className="text-sm cursor-pointer truncate flex-1">{path}</Label>
                    {hasSkill ? (
                      <Badge variant="outline" className="text-xs text-warning shrink-0">
                        {t('skills.copyToProject.installed')}
                      </Badge>
                    ) : null}
                  </div>
                );
              })}
              {selectedExistingCount > 0 ? (
                <div className="flex items-start gap-1.5 rounded-md bg-warning/10 px-2.5 py-2 mt-2">
                  <AlertTriangle className="h-3.5 w-3.5 shrink-0 mt-px text-warning" />
                  <p className="text-xs text-warning leading-relaxed">
                    {t('skills.copyToProject.overwriteWarning', { count: selectedExistingCount })}
                  </p>
                </div>
              ) : null}
            </>
          ) : (
            <p className="text-sm text-muted-foreground py-4 text-center">
              {t('skills.copyToProject.noProjects')}
            </p>
          )}
        </div>

        <DialogFooter className="mt-4">
          <Button variant="outline" onClick={onClose} disabled={copying}>
            {t('common.cancel')}
          </Button>
          <Button
            onClick={handleCopy}
            disabled={writeBlocked || copying || selected.size === 0}
          >
            {copying ? (
              <>
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
                {t('common.loading')}
              </>
            ) : (
              t('skills.copyToProject.copy', { count: selected.size })
            )}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
});
