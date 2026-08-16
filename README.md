<div align="center">
  <img src="src-tauri/app-icon.png" alt="Skill Deck" width="96">

  <h1>Skill Deck</h1>

  <p>
    <strong>A cross-platform desktop app for managing AI Agent Skills</strong>
  </p>

  <p>
    Manage Skills across global and project locations, track their sources and updates,<br>
    and control which agents can read them.
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

- **Discover and install** — Browse available Skills, review their sources, documentation, and security information, or install from GitHub, Git, local directories, Well-known URLs, raw `SKILL.md` files, and ZIP/tar archives
- **Browse and maintain** — Read installed Skill content, check and apply updates, and select a new source when the saved source no longer works
- **Projects and agents** — View global and project Skills together, filter by agent, manage which agents can read a Skill, and copy Project Skills to other projects; built-in definitions include Grok Build, Kimchi, MiniMax Code, and ZCode
- **Cross-platform management** — Manage Skills on Windows, macOS, and Linux; Windows users can also switch to installed WSL distributions

---

## Screenshots

<p align="center">
  <img src="docs/images/skill_selected.png" alt="Skill detail view" width="900">
</p>
<p align="center"><em>Browse installed Skills, read their full content, and check for or apply updates.</em></p>

<table>
  <tr>
    <td width="50%" align="center">
      <img src="docs/images/skills.png" alt="Skills overview">
      <br />
      <em>Browse Global and Project Skills separately and filter them by agent.</em>
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

- **Windows**: `skill-deck_x.x.x_windows_x64-setup.exe`; stable releases also provide `skill-deck_x.x.x_windows_x64.msi`
- **macOS Apple Silicon**: `skill-deck_x.x.x_macos_aarch64.dmg`
- **macOS Intel**: `skill-deck_x.x.x_macos_x64.dmg`
  > macOS builds are currently unsigned. If macOS blocks the app after installation, run:
  > ```bash
  > sudo xattr -rd com.apple.quarantine "/Applications/Skill Deck.app"
  > ```
- **Linux**: `skill-deck_x.x.x_linux_amd64.deb`, `skill-deck_x.x.x_linux_x86_64.rpm`, or `skill-deck_x.x.x_linux_amd64.AppImage`

---

## 🚀 Quick start

### 1. Choose an install entry point

Choose a Skill from the Discover page, or open the install entry in Global Skills or a target project. When installing a Project Skill, add or select the project in the sidebar first.

### 2. Provide a Skill source

Choose an online result or enter a Skill source. For example:

- `https://github.com/vercel-labs/skills`
- `vercel-labs/skills` (GitHub shorthand)
- `https://example.com/SKILL.md` or a ZIP/tar archive URL
- `/path/to/local/skill` (local path)

You can also paste a [`skills` CLI](https://github.com/vercel-labs/skills) `skills add` command. Skill Deck parses its source, Skill names, and target agents, then lets you confirm and adjust them before installation:

```bash
npx skills add vercel-labs/agent-skills --skill frontend-design -a claude-code
```

### 3. Confirm and install

Select the Skills, target agents, and installation mode, review the change preview, and run the installation. Cross-host download redirects require explicit confirmation. Direct downloads are one-time, new-install sources and do not provide update, reinstall, or source-repair actions afterward. After installation, the selected agents can read the Skill; you can continue reading and managing agents from the Skills workspace.

---

## 📚 Docs and feedback

- Read the [changelog](./CHANGELOG.md) for user-visible changes in each release
- Report a problem or suggest an improvement through [GitHub Issues](https://github.com/hccake/skill-deck/issues)
- See the [project documentation](./docs/README.md) for product behavior, compatibility, and development conventions

---

## 🛠️ Development and contribution

See the [contribution guide](./CONTRIBUTING.md) for the development environment, dependency versions, validation requirements, and contribution workflow.

```bash
git clone https://github.com/hccake/skill-deck.git
cd skill-deck
pnpm install --frozen-lockfile
pnpm tauri dev
```

Use `pnpm tauri build` for a production build. Build output is located at `src-tauri/target/release/bundle/`.

---

## 📄 License

[Apache License](LICENSE)

---

## 🙏 Acknowledgments

- [vercel-labs/skills](https://github.com/vercel-labs/skills) — A widely used third-party Skill manager and a compatibility and product reference for Skill Deck
- [Tauri](https://tauri.app/) — Cross-platform desktop app framework
- [Linux.do](https://linux.do/) — Community support and feedback
