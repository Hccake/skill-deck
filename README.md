<div align="center">
  <img src="src-tauri/app-icon.png" alt="Skill Deck" width="96">

  <h1>Skill Deck</h1>

  <p>
    <strong>A cross-platform desktop app for managing AI Agent Skills</strong>
  </p>

  <p>
    Install and maintain Skills across projects and agents.<br>
    Organize reusable Skill Libraries and read documentation with Simplified Chinese translations.
  </p>

  <p>
    <a href="https://github.com/hccake/skill-deck/releases/latest">
      <img src="https://img.shields.io/github/v/release/hccake/skill-deck" alt="Latest release">
    </a>
    <a href="https://github.com/hccake/skill-deck/actions/workflows/quality.yml">
      <img src="https://github.com/hccake/skill-deck/actions/workflows/quality.yml/badge.svg?branch=main" alt="Build status">
    </a>
    <a href="LICENSE">
      <img src="https://img.shields.io/github/license/hccake/skill-deck" alt="License">
    </a>
  </p>

  <p>
    <a href="https://github.com/hccake/skill-deck/releases/latest">Download latest release</a>
    ·
    <a href="#-quick-start">Quick start</a>
    ·
    <a href="README.zh-CN.md">中文</a>
  </p>
</div>

---

## ✨ Core capabilities

- **Discover and install**: Browse available Skills with their sources, documentation, and available audit information. Install from GitHub, GitLab, other Git repositories, local directories, Well-known URLs, raw `SKILL.md` files, or ZIP, tar, and tar.gz downloads.
- **Browse and maintain**: Read, update, and remove installed Skills, including installations found only in Agent directories. Skills without source records remain available for local management.
- **Skill Libraries**: Maintain reusable collections, apply them in priority order to global or project locations, and synchronize member changes. Reapply a library configuration when managed links need repair.
- **Projects and agents**: View global and project Skills, filter by agent, adjust associations, and copy Project Skills to other projects. Use the included agent definitions or add your own.
- **Bilingual reading**: Read original content alongside Simplified Chinese translations in Skills, Library members, and Discover. Choose Google, Microsoft, or an OpenAI-compatible service, and customize translation colors.
- **Cross-platform management**: Use Skill Deck on Windows, macOS, and Linux. Windows users can enable WSL support to manage Skills in installed WSL 2 distributions.

The app provides English and Chinese interfaces, light and dark themes, and restores the main window's size, position, and maximized state when reopened.

---

## 🖼️ Screenshots

<p align="center">
  <img src="docs/images/skill_selected.png" alt="Skill detail view" width="900">
</p>
<p align="center"><em>Browse installed Skills, read their full content, and check for or apply updates.</em></p>

<table>
  <tr>
    <td width="50%" align="center">
      <img src="docs/images/skills.png" alt="Skills overview">
      <br />
      <em>View global and project Skills and filter them by agent.</em>
    </td>
    <td width="50%" align="center">
      <img src="docs/images/discover.png" alt="Discover page">
      <br />
      <em>Browse installable Skills with source details, documentation, and security information.</em>
    </td>
  </tr>
</table>

<table>
  <tr>
    <td width="50%" align="center">
      <img src="docs/images/agent_manage.png" alt="Manage agents">
      <br />
      <em>Adjust agent associations for an installed Skill.</em>
    </td>
    <td width="50%" align="center">
      <img src="docs/images/copy.png" alt="Copy across projects">
      <br />
      <em>Copy a Project Skill to projects in the current or another Environment.</em>
    </td>
  </tr>
</table>

---

## 📥 Installation

Download the latest release from [GitHub Releases](https://github.com/hccake/skill-deck/releases/latest) and choose the installer for your platform:

| Platform | Installer |
|---|---|
| Windows x64 | `skill-deck_x.x.x_windows_x64-setup.exe`; stable releases also provide `skill-deck_x.x.x_windows_x64.msi` |
| macOS Apple Silicon | `skill-deck_x.x.x_macos_aarch64.dmg` |
| macOS Intel | `skill-deck_x.x.x_macos_x64.dmg` |
| Linux x64 | `skill-deck_x.x.x_linux_amd64.deb`, `skill-deck_x.x.x_linux_x86_64.rpm`, or `skill-deck_x.x.x_linux_amd64.AppImage` |

The `x.x.x` placeholder represents the selected version. Windows prereleases include only the `.exe` installer.

macOS installers currently lack an Apple Developer ID signature. After confirming that your download came from this project's Release page, you can use **System Settings > Privacy & Security > Open Anyway** if macOS blocks opening the app. See [Apple's instructions](https://support.apple.com/en-us/102445).

---

## 🚀 Quick start

### 1. Choose an install entry point

Choose Add in the global or project section of the Skills page to install at that location; add a project to Skill Deck before using it as a target. You can also select a Skill in Discover and choose Install. The wizard defaults to the global location and lets you select an added project instead. On Windows with WSL enabled, switch to the target distribution before starting.

### 2. Provide a Skill source

When starting from Skills, search online or enter a source. Discover pre-fills the selected Skill's source. Examples of manual input:

- `https://github.com/vercel-labs/skills`
- `vercel-labs/skills` (GitHub shorthand)
- `https://example.com/SKILL.md` or a ZIP/tar archive URL
- `/path/to/local/skill` or `C:\path\to\skill` (a local directory on the corresponding system)

You can also paste a [`skills` CLI](https://github.com/vercel-labs/skills) `skills add` command. Skill Deck parses its source, Skill names, and target agents, then lets you confirm and adjust them before installation:

```bash
npx skills add vercel-labs/agent-skills --skill web-design-guidelines -a claude-code
```

### 3. Confirm and install

Select the Skills, agent associations, and installation mode. Review the actual locations, existing content to replace, and source notices, then confirm. Downloads that redirect to another host require confirmation of the final host. Changing the main window's location does not change an open wizard's target.

After installation, use Skills to read content, manage agents, check for updates, or remove a Skill. To maintain a separate collection, add members on the Libraries page, then apply it through Manage Skill Libraries in the global or project section.

### Supported behavior and limits

- Skills without source records and Skills from local directories remain locally manageable and do not participate in remote update checks. Private GitHub source checks can use existing local Git credentials.
- Direct downloads support new installations without a same-name conflict and do not provide updates. Remove the existing Skill before installing it again.
- Updates preserve existing agent associations and show the affected locations before confirmation. Common content is always included, independent copies can be selected, and links follow their target content. Same-name Library versions are maintained in their own libraries.
- Directory links within a source or oversized sources may prevent an update. The app explains the reason and preserves installed content. See [update checking](./docs/update-checking.md) for the full conditions and [Skill lifecycle](./docs/skill-lifecycle.md) for installation and maintenance rules.

---

## 📚 Docs and feedback

- Read the [changelog](./CHANGELOG.md) for user-visible changes in each release
- Report a problem or suggest an improvement through [GitHub Issues](https://github.com/hccake/skill-deck/issues)
- See the [project documentation](./docs/README.md) for product behavior, compatibility, and development conventions

---

## 🛠️ Development and contribution

See the [contribution guide](./CONTRIBUTING.md) for environment setup, dependency versions, local development, validation, and submissions. Windows developers must prepare the Worker resources described there before running the app.

Use `pnpm tauri build` for a production build. Build output is located at `src-tauri/target/release/bundle/`.

---

## 📄 License

[Apache License 2.0](LICENSE)

---

## 🙏 Acknowledgments

- [vercel-labs/skills](https://github.com/vercel-labs/skills): An independently maintained Skill manager and a compatibility and product reference for Skill Deck.
- [Tauri](https://tauri.app/): Cross-platform desktop app framework.
- [Linux.do](https://linux.do/): Community support and feedback.
