# AI 用量监控

Windows 系统托盘小工具，实时展示本机已安装的 AI 编程工具的用量/配额：**Claude Code**、**Codex CLI**、**Cursor**、**Amp**、**Factory Droid**。

托盘图标常驻，点击弹出小窗，各工具的用量、限额、重置倒计时一目了然；某个工具取数失败或未登录，不影响其他工具正常显示。

## 功能

- 托盘弹窗展示各工具当前用量、限额百分比、重置倒计时
- 设置窗口可选择要监控哪些工具、刷新间隔、是否开机自启
- 后台按设定间隔自动轮询，也可手动立即刷新

## 技术栈

Tauri v2（Rust）+ React 19 + TypeScript + Vite

## 开发

```bash
npm install
npm run tauri dev
```

## 构建安装包

```bash
npm run tauri build
```

`tauri build` 只负责打包，不会自动启动应用。构建完成后产物在 `src-tauri/target/release/`：

- 直接双击 `tauri-app.exe` 即可运行
- 或去 `src-tauri/target/release/bundle/` 下找生成的安装包（`.msi` / nsis `.exe`），安装后才会正常注册到系统（如需开机自启也要走安装后的版本）

推荐 IDE：VS Code + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)

## 支持的工具及数据来源

| 工具 | 数据来源 |
|---|---|
| Claude Code | `~/.claude/.credentials.json`（OAuth）+ Anthropic usage API |
| Codex CLI | `~/.codex/auth.json` |
| Cursor | 本地 SQLite（`globalStorage/state.vscdb`） |
| Amp | `amp` CLI 输出解析 |
| Factory Droid | `~/.factory/auth.v2.*` |

## 已知限制

目前仅在 Windows 上开发和验证；`amp` 二进制探测与 Claude 凭据读取在 macOS 上存在已知的兼容性缺口（详见 `CLAUDE.md`）。
