/* @vitest-environment jsdom */

import '@/test-utils';
import { describe, expect, it, vi } from 'vitest';
import { render } from '@testing-library/react';
import { CompactSkillList } from '../CompactSkillList';
import type { InstalledSkill } from '@/bindings';

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}));

function makeSkill(name: string): InstalledSkill {
  return {
    name,
    description: `${name} description`,
    path: `/tmp/${name}`,
    canonicalPath: `/tmp/.agents/${name}`,
    scope: 'global',
    agents: ['codex'],
    source: 'owner/repo',
    sourceUrl: 'https://github.com/owner/repo',
    installedAt: null,
    updatedAt: null,
    hasUpdate: false,
    pluginName: null,
    gitRef: null,
  };
}

describe('CompactSkillList', () => {
  it('stretches its scroll area to the full available panel size', () => {
    const { container } = render(
      <div className="h-[480px]">
        <CompactSkillList
          globalSkills={[makeSkill('alpha'), makeSkill('beta')]}
          projectSkills={[]}
          selectedSkillName="alpha"
          selectedSkillScope="global"
          isProjectSelected={false}
          projectTitle="Project Skills"
          onSkillClick={() => undefined}
        />
      </div>
    );

    const scrollArea = container.querySelector('[data-slot="scroll-area"]');
    const viewport = container.querySelector('[data-slot="scroll-area-viewport"]');

    expect(scrollArea).not.toBeNull();
    expect(scrollArea?.className).toContain('absolute');
    expect(scrollArea?.className).toContain('inset-0');
    expect(scrollArea?.className).toContain('w-full');
    expect(scrollArea?.className).toContain('h-full');
    expect(viewport).not.toBeNull();
    expect(viewport?.className).toContain('[&>div]:!block');
    expect(viewport?.className).toContain('[&>div]:w-full');
    expect(viewport?.className).toContain('[&>div]:min-w-0');
  });
});
