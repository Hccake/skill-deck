// src/pages/SkillsPage.tsx
import { ContextSidebar, SkillsPanel } from '@/components/skills';

export function SkillsPage() {
  return (
    <div className="flex h-full">
      {/* Left Sidebar: Context */}
      <ContextSidebar />

      {/* Right Panel: Skills List */}
      <SkillsPanel />
    </div>
  );
}
