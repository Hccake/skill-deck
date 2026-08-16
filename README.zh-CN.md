<div align="center">
  <img src="src-tauri/app-icon.png" alt="Skill Deck" width="96">

  <h1>Skill Deck</h1>

  <p>
    <strong>跨平台的 AI Agent Skill 管理桌面应用</strong>
  </p>

  <p>
    集中管理全局与项目 Skill，跟踪来源和更新，并管理哪些 Agent 能够读取它们。
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
    <a href="https://github.com/hccake/skill-deck/releases/latest">下载最新版本</a>
    ·
    <a href="#-快速开始">快速开始</a>
    ·
    <a href="README.md">English</a>
  </p>
</div>

---

## ✨ 核心能力

- **发现与安装**：从在线目录发现 Skill，查看来源、说明和安全信息，或者从 GitHub、Git、本地目录、约定地址（Well-known 地址）、原始 `SKILL.md` 文件和 ZIP、tar 归档安装
- **浏览与维护**：阅读已安装 Skill 的完整内容，检查并执行更新，在来源失效时重新选择来源
- **项目与 Agent 管理**：统一查看全局与项目 Skill，按 Agent 筛选，调整 Skill 可供哪些 Agent 使用，并在项目之间复制 Skill；随应用提供 Grok Build、Kimchi、MiniMax Code 和 ZCode 等 Agent 信息
- **跨平台管理**：在 Windows、macOS 和 Linux 上管理 Skill；Windows 用户还可以切换到已安装的 WSL 发行版

---

## 🖼️ 界面预览

<p align="center">
  <img src="docs/images/skill_selected.png" alt="Skill 详情视图" width="900">
</p>
<p align="center"><em>浏览已安装的 Skill，查看完整内容，并检查或执行更新。</em></p>

<table>
  <tr>
    <td width="50%" align="center">
      <img src="docs/images/skills.png" alt="Skills 工作台">
      <br />
      <em>分别查看全局 Skill 与项目 Skill，并按 Agent 筛选。</em>
    </td>
    <td width="50%" align="center">
      <img src="docs/images/discover.png" alt="Discover 页面">
      <br />
      <em>浏览可安装的 Skill，并查看来源、说明和安全信息。</em>
    </td>
  </tr>
</table>

<table>
  <tr>
    <td width="50%" align="center">
      <img src="docs/images/agent_manage.png" alt="管理 Agent">
      <br />
      <em>调整哪些 Agent 能够读取已安装的 Skill。</em>
    </td>
    <td width="50%" align="center">
      <img src="docs/images/copy.png" alt="复制到其他项目">
      <br />
      <em>将项目 Skill 复制到当前或其他 Environment 中的项目。</em>
    </td>
  </tr>
</table>

---

## 📥 下载与安装

从 [GitHub Releases](https://github.com/hccake/skill-deck/releases/latest) 下载最新版本，并选择对应平台的安装包：

- **Windows**：`skill-deck_x.x.x_windows_x64-setup.exe`；稳定版本还会提供 `skill-deck_x.x.x_windows_x64.msi`
- **macOS Apple Silicon**：`skill-deck_x.x.x_macos_aarch64.dmg`
- **macOS Intel**：`skill-deck_x.x.x_macos_x64.dmg`
  > macOS 构建目前没有 Apple 开发者签名。如果安装后被系统拦截，可执行：
  > ```bash
  > sudo xattr -rd com.apple.quarantine "/Applications/Skill Deck.app"
  > ```
- **Linux**：`skill-deck_x.x.x_linux_amd64.deb`、`skill-deck_x.x.x_linux_x86_64.rpm` 或 `skill-deck_x.x.x_linux_amd64.AppImage`

---

## 🚀 快速开始

### 1. 选择安装入口

可以从“发现”页面选择 Skill，也可以在“全局 Skill”或目标项目中打开安装入口。安装项目 Skill 时，先在侧栏添加或选择对应项目。

### 2. 提供 Skill 来源

选择在线搜索结果，或者输入需要安装的 Skill 来源，例如：

- `https://github.com/vercel-labs/skills`
- `vercel-labs/skills`（GitHub 简写）
- `https://example.com/SKILL.md` 或 ZIP、tar 归档地址
- `/path/to/local/skill`（本地目录）

也可以直接粘贴 [`skills` CLI](https://github.com/vercel-labs/skills) 的 `skills add` 安装命令。Skill Deck 会解析其中的来源、Skill 和目标 Agent，并在安装前提供确认和调整：

```bash
npx skills add vercel-labs/agent-skills --skill frontend-design -a claude-code
```

### 3. 确认并安装

选择需要安装的 Skill、目标 Agent 和安装方式，检查变更预览，然后执行安装。下载地址发生跨主机重定向时，需要明确确认最终下载主机。直接下载属于一次性全新安装来源，安装后不提供更新、重新安装或来源修复操作。安装完成后，关联的 Agent 即可读取该 Skill；你也可以在 Skills 工作台中继续阅读和管理 Agent。

---

## 📚 文档与反馈

- 查看[版本记录](./CHANGELOG.md)了解各版本的用户可见变化
- 通过 [GitHub Issues](https://github.com/hccake/skill-deck/issues) 报告问题或提出建议
- 从[项目文档](./docs/README.md)了解产品行为、兼容范围和开发约定

---

## 🛠️ 开发与贡献

开发环境、依赖版本、验证要求和贡献流程见[贡献指南](./CONTRIBUTING.md)。

```bash
git clone https://github.com/hccake/skill-deck.git
cd skill-deck
pnpm install --frozen-lockfile
pnpm tauri dev
```

生产构建使用 `pnpm tauri build`，构建产物位于 `src-tauri/target/release/bundle/`。

---

## 📄 许可证

[Apache License 2.0](LICENSE)

---

## 🙏 致谢

- [vercel-labs/skills](https://github.com/vercel-labs/skills) — 被广泛使用的第三方 Skill 管理工具，也是 Skill Deck 的兼容与产品参考
- [Tauri](https://tauri.app/) — 跨平台桌面应用框架
- [Linux.do](https://linux.do/) — 社区支持与反馈
