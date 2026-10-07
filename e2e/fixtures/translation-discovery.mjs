// 翻译桌面验收只使用固定公开文本，真实翻译请求仍由 Rust 发起。
const summary = {
  slug: 'translation-guide', name: 'Translation guide', source: 'example/skills',
  summary: 'Read the instructions before making changes.', installs: 1000,
  displayMetric: { kind: 'installs', rawText: '1K', sortValue: 1000 },
  isOfficial: false, detailUrl: 'https://www.skills.sh/example/skills/translation-guide',
};
export async function getDiscoverLeaderboard() { return [summary]; }
export async function searchDiscoverSkills() { return [summary]; }
export async function getDiscoverSkillDetail() {
  return { ...summary, description: summary.summary,
    summaryHtml: '<p>Read the instructions before making changes.</p>',
    highlights: [], securityAudits: [], installedOn: [],
    contentHtml: '<h2>Installation</h2><p>Run <code>npm install</code> to install dependencies.</p><ul><li>Read the guide</li><li>Check the examples</li></ul><blockquote>Read the notes<p>Read the guide before installing.</p></blockquote><pre>Read diagram\nA -&gt; B</pre>',
  };
}
