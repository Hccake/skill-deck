<div align="center">
  <!-- TODO: Add Logo -->
  <!-- <img src="docs/images/logo.svg" alt="Skill Deck Logo" width="120"> -->
  <h1>Skill Deck</h1>
  <p>
    <strong>A desktop Skill manager that works alongside skills CLI.</strong>
  </p>

  <p>
    <img src="https://img.shields.io/badge/Tauri-v2-blue" alt="Tauri v2">
    <img src="https://img.shields.io/badge/React-19-61dafb" alt="React 19">
    <img src="https://img.shields.io/badge/skills%20CLI-compatible-green" alt="skills CLI compatible">
  </p>

  <a href="README.zh-CN.md">中文</a>
</div>

---

Skill Deck is a cross-platform desktop application for browsing, installing, reading, updating, copying, and removing Skills used by AI agents. It also manages how Skills are made available to agents.

[`skills` CLI](https://github.com/vercel-labs/skills) is a third-party tool independently maintained by the `vercel-labs/skills` project. Skill Deck can read the Skill directories and compatible lock data it uses while implementing all runtime capabilities independently. Use Skill Deck on its own or alongside the CLI.

**Key highlights:**
- **Cross-platform desktop app** — Windows, macOS, and Linux, with optional WSL environments on Windows
- **Works alongside skills CLI** — Both tools can read and write the same Skill directories and compatible lock data
- **Complete Skill workflows** — Installation, updates, source repair, project copy, agent management, and in-app updates

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

## ✨ Features

- 🗂 **Skills workspace** — Browse installed Skills with their content, sources, update status, and associated agents
- 🧠 **Agent filtering and management** — Filter Skills by agent and add or remove agent access for installed Skills
- 🌍 **Global and Project locations** — Manage Global Skills and each project's Project Skills in the current Environment
- 🐧 **Optional WSL Environments** — Switch between the Native Environment and installed WSL distributions on Windows
- 🔍 **Discover and install** — Use GitHub, Git, local paths, Well-known URLs, or `skills add` commands
- ♻️ **Updates and source repair** — Check for updates and select a new source when saved source information no longer works
- 📋 **Cross-project copy** — Copy a Project Skill to one or more target projects
- 📦 **Two installation modes** — Choose symbolic links or file copies for each target
- 🧩 **Custom agent support** — Add Skill locations and detection conditions for agents not included with Skill Deck
- 🌐 **Bilingual interface** — Use Simplified Chinese or English in the main window and installation wizard
- ⚙️ **Proxy settings** — Connect online services directly or through a custom proxy, with explicit Native and per-distribution WSL Git behavior
- 🔄 **In-app updates** — Check, download, and install new versions from GitHub Releases

Skill availability is determined by its installation location and agent associations.

---

## 📦 Installation

### Option 1: Download pre-built binaries (recommended)

Download the installer for your platform from [GitHub Releases](https://github.com/hccake/skill-deck/releases):

- **Windows**: `skill-deck_x.x.x_windows_x64-setup.exe` or `skill-deck_x.x.x_windows_x64.msi`
- **macOS Apple Silicon**: `skill-deck_x.x.x_macos_aarch64.dmg`
- **macOS Intel**: `skill-deck_x.x.x_macos_x64.dmg`
  > macOS builds are currently unsigned. If macOS blocks the app after installation, run:
  > ```bash
  > sudo xattr -rd com.apple.quarantine "/Applications/Skill Deck.app"
  > ```
- **Linux**: `skill-deck_x.x.x_linux_amd64.deb`, `skill-deck_x.x.x_linux_x86_64.rpm`, or `skill-deck_x.x.x_linux_amd64.AppImage`

### Option 2: Build from source

See the [contribution guide](CONTRIBUTING.md#开发环境) for the current development environment. Node.js, pnpm, and Rust versions follow CI, `package.json`, and `src-tauri/Cargo.toml`.

```bash
# Clone the repo
git clone https://github.com/hccake/skill-deck.git
cd skill-deck

# Install dependencies
pnpm install --frozen-lockfile

# Run in development mode
pnpm tauri dev

# Build for production
pnpm tauri build
```

Build output is located at `src-tauri/target/release/bundle/`.

---

## 🚀 Quick Start

### 1. Add a project

Use the add button in the Projects section of the sidebar and select the project directory to manage. Skip this step when managing Global Skills.

### 2. Prepare a Skill source

Copy the source of the Skill you want to install. For example:
- `https://github.com/vercel-labs/skills`
- `vercel-labs/skills` (GitHub shorthand)
- `/path/to/local/skill` (local path)

You can also paste a supported `skills` CLI install command. Skill Deck parses its source, Skill names, and target agents:

```bash
npx skills add vercel-labs/agent-skills --skill frontend-design -a claude-code
```

### 3. Install a Skill

Use the add action for Global Skills or the target project, enter the Skill source, select the Skills, target agents, and installation mode, then confirm the preview and run the installation. When a CLI command is pasted, the wizard preselects its `--skill` and `--agent` options; you can still adjust them before confirmation.

### 4. Use with an agent

After installation, each selected agent reads the Skill from the standard Skill directory or its own Skill directory, depending on its definition and the selected installation mode.

---

## 📄 License

[Apache License](LICENSE)

---

## 🙏 Acknowledgments

- [vercel-labs/skills](https://github.com/vercel-labs/skills) — A widely used third-party Skill manager and a compatibility and product reference for Skill Deck
- [Tauri](https://tauri.app/) — Cross-platform desktop app framework
- [Linux.do](https://linux.do/) — Community support and feedback
