import { Bot } from 'lucide-react';
import { cn } from '@/lib/utils';
import type { AgentType } from '@/bindings';

interface AgentIconProps {
  agentId?: AgentType | string;
  className?: string;
  iconClassName?: string;
}

export function AgentIcon({ className, iconClassName }: AgentIconProps) {
  // Currently using a unified generic icon as per design requirement.
  // In the future, we can map `agentId` to specific SVG icons or different lucide icons here.

  return (
    <div
      className={cn(
        "flex shrink-0 items-center justify-center rounded-md bg-muted/60 border border-border/50",
        className
      )}
    >
      <Bot className={cn("h-4 w-4 text-foreground/70", iconClassName)} />
    </div>
  );
}
