<div align="center">
  <!-- TODO: Add Logo -->
  <!-- <img src="docs/images/logo.svg" alt="Skill Deck Logo" width="120"> -->
  <h1>Skill Deck</h1>
  <p>
    <strong>A native desktop UI compatible with the skills CLI.</strong>
  </p>

  <p>
    <img src="https://img.shields.io/badge/Tauri-v2-blue" alt="Tauri v2">
    <img src="https://img.shields.io/badge/React-19-61dafb" alt="React 19">
    <img src="https://img.shields.io/badge/skills%20CLI-v1.4.7-green" alt="skills CLI v1.4.7">
  </p>

  <img src="docs/images/screenshot-main.png" alt="Skill Deck Main UI" width="800">

  <a href="README.zh-CN.md">中文</a>
</div>

---

Skill Deck is a lightweight, native desktop application for managing and exploring **Skills**—a graphical companion to [`vercel-labs/skills`](https://github.com/vercel-labs/skills).

**Key highlights:**
- **Native Rust implementation** — Does not invoke the `skills` CLI binary, no Node.js required
- **Fully compatible** — Uses the same configuration format; CLI and GUI can be used interchangeably
- **Companion, not replacement** — Switch freely between CLI and GUI, or use both side by side

The goal is simple: make Skills easier to inspect, understand, and apply across projects and editors—without changing how they work.

---

## ✨ Features

- 🗂 **Unified view** — Browse all installed Skills in one place
- 🌍 **Global & project scope** — Manage Skills at global level or per-project
- 🧠 **Clear visibility** — Understand where each Skill is applied at a glance
- 🔄 **Multi-editor support** — Auto-detect installed editors (VS Code, Cursor, Windsurf, etc.) and sync Skills across them
- 📦 **Dual install modes** — Choose between Symlink and Copy when installing Skills
- 🔍 **Discover & install** — Install Skills from GitHub repos or local paths
- 🌐 **Bilingual UI** — English and Chinese interface
- ⚡ **Fast & lightweight** — Built with Tauri v2, fast startup, low resource usage

> ⚠️ Skill disabling is not supported by the underlying model.
> Skills can be installed or removed only.

---

## 📦 Installation

### Option 1: Download pre-built binaries (recommended)

Download the installer for your platform from [GitHub Releases](https://github.com/hccake/skill-deck/releases):

- **Windows**: `Skill-Deck_x.x.x_x64_en-US.msi`
- **macOS**: `Skill-Deck_x.x.x_universal.dmg` (Intel + Apple Silicon)
  > **Note for macOS users**: The application is currently unsigned. If you encounter an "app is damaged" or "cannot be opened because the developer cannot be verified" error after installation, please run the following command in your terminal:
  > ```bash
  > sudo xattr -rd com.apple.quarantine "/Applications/Skill Deck.app"
  > ```
- **Linux**: `skill-deck_x.x.x_amd64.deb` or `skill-deck-x.x.x-1.x86_64.rpm`

### Option 2: Build from source

**Prerequisites**:
- Node.js >= 18
- pnpm >= 8
- Rust >= 1.70
- System dependencies: see [Tauri Prerequisites](https://tauri.app/v2/guides/prerequisites)

```bash
# Clone the repo
git clone https://github.com/hccake/skill-deck.git
cd skill-deck

# Install dependencies
pnpm install

# Run in development mode
pnpm tauri dev

# Build for production
pnpm tauri build
```

Build output is located at `src-tauri/target/release/bundle/`.

---

## 🚀 Quick Start

### 1. Add a project

Click the `+` button next to "Projects" in the sidebar and select your code project directory.

### 2. Prepare a Skill source

Find the GitHub repo URL or local path of the Skill you want to install. For example:
- `https://github.com/vercel-labs/skills`
- `vercel-labs/skills` (GitHub shorthand)
- `/path/to/local/skill` (local path)

You can also paste a `skills` CLI install command directly — Skill Deck will automatically parse the source, skill names, and target agents from it:

```bash
npx skills add vercel-labs/agent-skills --skill frontend-design -a claude-code
```

### 3. Install a Skill

Click `+ Add` next to "Global Skills" or any project → enter the Skill source (or paste a CLI command) → select target editors (VS Code / Cursor, etc.) → choose install mode (Symlink / Copy) → confirm.

When a CLI command is pasted, the `--skill` and `--agent` options are automatically pre-selected in the wizard. You can still modify the selections before confirming.

### 4. Use in your editor

Once installed, open the project in the corresponding editor. The Skill will be automatically loaded by the AI assistant.

---

## 📄 License

[MIT License](LICENSE)

---

## 🙏 Acknowledgments

- [vercel-labs/skills](https://github.com/vercel-labs/skills) — The original CLI tool
- [Tauri](https://tauri.app/) — Cross-platform desktop app framework
