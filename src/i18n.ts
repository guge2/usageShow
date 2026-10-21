import type { LanguageSetting } from "./types";

export type Lang = "en" | "zh";

export function resolveLang(setting: LanguageSetting | undefined): Lang {
  if (setting === "en" || setting === "zh") return setting;
  return navigator.language.toLowerCase().startsWith("zh") ? "zh" : "en";
}

/** English text is the key; only Chinese needs a table. */
const ZH: Record<string, string> = {
  // panel
  "AI Usage Monitor": "AI 用量监控",
  Refresh: "刷新",
  "Refresh this card": "刷新此卡片",
  Settings: "设置",
  "Loading...": "加载中...",
  Used: "已用",
  Left: "剩余",
  "Show how much is left": "切换为显示剩余额度",
  "Show how much is used": "切换为显示已用额度",
  "Last updated {time}": "更新于 {time}",
  "Not connected": "未连接",
  "Not connected: {names}": "未连接：{names}",
  Error: "错误",
  "Not used yet": "尚未使用",
  "Resets {when}": "{when} 重置",
  "Reset time passed": "已过重置时间",
  "in {n}m": "{n} 分钟后",
  "in {n}h": "{n} 小时后",
  "in {n}d": "{n} 天后",
  "tomorrow {time}": "明天 {time}",
  "Just now": "刚刚",
  "{n}m ago": "{n} 分钟前",
  "{n}h ago": "{n} 小时前",
  "{n}d ago": "{n} 天前",
  "Updated {when}": "{when}更新",
  "As of {when}": "截至 {when}",
  "Showing the last reading; the latest refresh failed: {why}": "显示的是上次读数；最近一次刷新失败：{why}",
  "A window has passed its reset time; the current allowance is unknown":
    "有窗口已过重置时间，当前额度未知",
  "Pace {n}": "配速 {n}",
  "{n} more used than an even pace would have by now — at this rate it runs out before it resets":
    "比匀速多用了 {n}，按此速度会在重置前用完",
  "{n} less used than an even pace would have by now — at this rate it lasts until it resets":
    "比匀速少用了 {n}，按此速度可以撑到重置",
  "Right on an even pace": "正好是匀速",
  "Runs out ~{when} at this pace": "按当前速度约 {when} 用完",
  "{label} runs out ~{when} at this pace": "{label} 按当前速度约 {when} 用完",
  "Lasts past the reset at this pace": "按当前速度可撑到重置",
  "{n} used": "已用 {n}",
  "{n} left": "剩余 {n}",
  "{amount} remaining": "剩余 {amount}",
  // window labels
  Monthly: "月度",
  Weekly: "每周",
  Credits: "积分",
  Standard: "标准",
  Premium: "高级",
  "Monthly requests": "月度请求",
  "Extra Usage": "额外用量",
  Primary: "主窗口",
  Secondary: "次窗口",
  // common backend messages
  "Rate limited, try again later": "请求过于频繁，请稍后再试",
  "Timed out - the API may be unreachable from this network; check the proxy setting":
    "请求超时，当前网络可能无法访问该接口，请检查代理设置",
  "Could not connect - check your network and the proxy setting": "无法连接，请检查网络和代理设置",
  "Request refused (403) - the API may be unreachable from this network; check the proxy setting":
    "请求被拒绝（403），当前网络可能无法访问该接口，请检查代理设置",
  "Received an unreadable response": "收到无法解析的响应",
  "Adapter crashed while fetching": "获取时适配器崩溃",
  "This plan does not expose usage figures": "该套餐不提供用量数据",
  "This account uses credit-based billing; this endpoint doesn't support showing balance yet":
    "该账号使用积分计费，暂不支持显示余额",
  "No usage data available": "暂无用量数据",
  "Could not locate home directory": "找不到用户目录",
  "AGY login is expired - open AGY once to refresh": "AGY 登录已过期，请打开一次 AGY 以刷新",
  "Failed to parse amp usage output": "无法解析 amp usage 输出",
  "No usage API": "无用量接口",
  "No tools enabled. Choose some in Settings.": "未启用任何工具，请在设置中选择。",
  "Extra usage": "额外用量",
  "Claude Code login not found": "未找到 Claude Code 登录信息",
  "Codex login not found": "未找到 Codex 登录信息",
  "Cursor installation not detected": "未检测到 Cursor",
  "Not logged in to Cursor": "未登录 Cursor",
  "Amp CLI not detected": "未检测到 Amp CLI",
  "Not logged in to Amp": "未登录 Amp",
  "Factory Droid login not found": "未找到 Factory Droid 登录信息",
  "AGY CLI not detected": "未检测到 AGY CLI",
  "AGY login not found": "未找到 AGY 登录信息",
  "Grok Build login not found": "未找到 Grok Build 登录信息",
  "No active usage window": "暂无用量窗口",
  "Failed to parse response": "响应解析失败",
  "Login expired - open Claude Code once to refresh": "登录已过期，请打开一次 Claude Code 以刷新",
  "Login expired - open Codex CLI once to refresh": "登录已过期，请打开一次 Codex CLI 以刷新",
  "Login expired - please sign in to Cursor again": "登录已过期，请重新登录 Cursor",
  "Login expired - open Droid CLI once to refresh": "登录已过期，请打开一次 Droid CLI 以刷新",
  "Login expired - please sign in to Droid CLI again": "登录已过期，请重新登录 Droid CLI",
  "Login expired - open Grok Build once to refresh": "登录已过期，请打开一次 Grok Build 以刷新",
  "Grok Build is not logged in - run `grok login`": "Grok Build 未登录，请运行 `grok login`",
  "Codex is not logged in with a ChatGPT account": "Codex 未使用 ChatGPT 账号登录",
  "Claude Code is not logged in via OAuth": "Claude Code 未通过 OAuth 登录",
  // settings
  Display: "显示",
  Language: "语言",
  "Follow system": "跟随系统",
  "Rings show": "圆环显示",
  "Used quota": "已用额度",
  "Remaining quota": "剩余额度",
  "Usage alerts": "用量提醒",
  "Notify when a window reaches": "用量达到时通知",
  Off: "关闭",
  "Each window alerts once per cycle.": "每个窗口在每个周期内只提醒一次。",
  "Refresh interval": "刷新间隔",
  "{n} minute": "{n} 分钟",
  "{n} minutes": "{n} 分钟",
  "Visible tools": "显示的工具",
  Startup: "启动",
  Network: "网络",
  Automatic: "自动",
  Direct: "直连",
  Manual: "手动",
  "Use HTTPS_PROXY/HTTP_PROXY, then the Windows system proxy.":
    "优先使用 HTTPS_PROXY/HTTP_PROXY，其次是 Windows 系统代理。",
  "Never use a proxy.": "从不使用代理。",
  "Always use the address below.": "始终使用下面的地址。",
  "Direct connection (no proxy)": "直接连接（不使用代理）",
  "Couldn't save settings: {e}": "保存设置失败：{e}",
  "Launch automatically at Windows startup": "开机时自动启动",
  Updates: "更新",
  Unknown: "未知",
  "Check GitHub Releases for a signed update.": "从 GitHub Releases 检查签名更新。",
  "Checking for updates...": "正在检查更新...",
  "You're using the latest version.": "已是最新版本。",
  "Version {v} is ready to install.": "新版本 {v} 可以安装。",
  "Couldn't check for updates: {e}": "检查更新失败：{e}",
  "Downloading version {v}...": "正在下载 {v}...",
  "Update installed. Restarting...": "更新已安装，正在重启...",
  "Couldn't install the update: {e}": "安装更新失败：{e}",
  "Checking...": "检查中...",
  "Downloading...": "下载中...",
  "Restarting...": "重启中...",
  "Install v{v}": "安装 v{v}",
  "Check for updates": "检查更新",
};

/** Messages with a variable tail, translated by prefix. */
const ZH_PREFIXES: [string, string][] = [
  ["Request failed: ", "请求失败："],
  ["API returned ", "接口返回 "],
  ["Environment variable: ", "环境变量："],
  ["Windows system proxy: ", "Windows 系统代理："],
  ["Manual: ", "手动："],
  ["Failed to read local database: ", "读取本地数据库失败："],
  ["Failed to parse credentials: ", "登录信息解析失败："],
];

/** Messages with a variable head (e.g. a plan name), translated by suffix. */
const ZH_SUFFIXES: [string, string][] = [
  [" - this plan does not expose usage figures", " · 该套餐不提供用量数据"],
];

export type T = (key: string, vars?: Record<string, string | number>) => string;

export function makeT(lang: Lang): T {
  return (key, vars) => {
    let text = lang === "zh" ? (ZH[key] ?? key) : key;
    if (lang === "zh" && text === key) {
      for (const [en, zh] of ZH_PREFIXES) {
        if (key.startsWith(en)) {
          text = zh + key.slice(en.length);
          break;
        }
      }
      for (const [en, zh] of ZH_SUFFIXES) {
        if (text.endsWith(en)) {
          text = text.slice(0, -en.length) + zh;
          break;
        }
      }
    }
    if (vars) {
      for (const [k, v] of Object.entries(vars)) text = text.split(`{${k}}`).join(String(v));
    }
    return text;
  };
}

/** Window labels like "7d · Opus": translate each part that has a translation. */
export function translateLabel(t: T, label: string): string {
  return label
    .split(" · ")
    .map((part) => t(part))
    .join(" · ");
}
