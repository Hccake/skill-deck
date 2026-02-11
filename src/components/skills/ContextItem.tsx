// src/components/skills/ContextItem.tsx
import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { MoreHorizontal, Trash2, Globe, Folder, FolderOpen } from 'lucide-react';
import { Button } from '@/components/ui/button';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from '@/components/ui/tooltip';
import { cn } from '@/lib/utils';

interface ContextItemProps {
  label: string;
  /** 完整路径（用于 Tooltip） */
  fullPath?: string;
  selected: boolean;
  onClick: () => void;
  isGlobal?: boolean;
  /** 在文件管理器中打开（仅 Project） */
  onOpenInExplorer?: () => void;
  /** 移除回调（仅 Project） */
  onRemove?: () => void;
}

export function ContextItem({
  label,
  fullPath,
  selected,
  onClick,
  isGlobal = false,
  onOpenInExplorer,
  onRemove,
}: ContextItemProps) {
  const { t } = useTranslation();
  const [isHovered, setIsHovered] = useState(false);
  const [isMenuOpen, setIsMenuOpen] = useState(false);

  const showMenu = !isGlobal && (onOpenInExplorer || onRemove);
  const showMenuButton = showMenu && (isHovered || isMenuOpen);

  const itemContent = (
    <div
      className={cn(
        'group flex items-center justify-between px-3 py-2 rounded-md cursor-pointer transition-colors',
        selected
          ? 'bg-accent text-accent-foreground'
          : 'hover:bg-accent/50 text-muted-foreground hover:text-foreground'
      )}
      onClick={onClick}
      onMouseEnter={() => setIsHovered(true)}
      onMouseLeave={() => setIsHovered(false)}
    >
      <div className="flex items-center gap-2 min-w-0">
        <span
          className={cn(
            'flex h-2 w-2 rounded-full flex-shrink-0',
            selected ? 'bg-primary' : 'border border-muted-foreground/50'
          )}
        />
        {isGlobal ? (
          <Globe className="h-4 w-4 flex-shrink-0" />
        ) : (
          <Folder className="h-4 w-4 flex-shrink-0" />
        )}
        <span className="text-sm truncate">{label}</span>
      </div>

      {showMenuButton && (
        <DropdownMenu open={isMenuOpen} onOpenChange={setIsMenuOpen}>
          <DropdownMenuTrigger asChild>
            <Button
              variant="ghost"
              size="icon"
              className="h-6 w-6 flex-shrink-0"
              onClick={(e) => e.stopPropagation()}
            >
              <MoreHorizontal className="h-4 w-4" />
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end">
            {onOpenInExplorer && (
              <DropdownMenuItem onClick={onOpenInExplorer}>
                <FolderOpen className="h-4 w-4 mr-2" />
                {t('context.openInExplorer')}
              </DropdownMenuItem>
            )}
            {onRemove && (
              <DropdownMenuItem
                onClick={onRemove}
                className="text-destructive focus:text-destructive"
              >
                <Trash2 className="h-4 w-4 mr-2" />
                {t('context.remove')}
              </DropdownMenuItem>
            )}
          </DropdownMenuContent>
        </DropdownMenu>
      )}
    </div>
  );

  // 非全局项目显示 Tooltip 展示完整路径
  if (fullPath && !isGlobal) {
    return (
      <Tooltip>
        <TooltipTrigger asChild>
          {itemContent}
        </TooltipTrigger>
        <TooltipContent side="right" className="max-w-xs">
          <p className="text-xs font-mono break-all">{fullPath}</p>
        </TooltipContent>
      </Tooltip>
    );
  }

  return itemContent;
}
