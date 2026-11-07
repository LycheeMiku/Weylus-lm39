# Weylus (Graphics Tablet Edition)

[![Release](https://img.shields.io/github/v/release/LycheeMiku/Weylus-lm39)](https://github.com/LycheeMiku/Weylus-lm39/releases)
[![License: AGPL v3](https://img.shields.io/badge/License-AGPL%20v3-blue.svg)](LICENSE)
[![Platform](https://img.shields.io/badge/Platform-Windows%20%7C%20iPadOS%20%7C%20Android-lightgrey)](#)

本项目是基于 [H-M-H/Weylus](https://github.com/H-M-H/Weylus) 的专属优化分支，专注于将 iPad (搭配 Apple Pencil) 或 Android 平板打造成一台**超低延迟、高响应、原生压感的“无屏数位板”**（Graphics Tablet）。

---

## 🌟 核心特色与重构改进

### 1. 纯粹数位板模式（极低延迟 & 零画面开销）
- **剥离屏幕镜像与视频编码**：彻底移除了原版繁重的 DXGI/桌面捕获与硬件编码串流（NVENC/MediaFoundation/VP9 等），不消耗电脑显卡编解码算力。
- **极致续航与发热控制**：iPad 端转为纯黑手写板，告别平板发热与掉电，书写延迟降至物理局域网极限（1~3ms）。

### 2. 原生 Windows Ink 笔指针注入
- **真正支持 Windows Ink 压感**：通过 Windows 底层 `InjectSyntheticPointerInput` API 原生注入笔交互。
- **全要素笔特性支持**：
  - 悬停检测（Hover）与笔尖落笔（Contact）
  - 平滑压感曲线（支持 1024 ~ 4096 级压感与压力灵敏度加权）
  - 笔身倾斜角度（Tilt X/Y）
  - 快捷切换：侧键功能与物理橡皮擦模式（Eraser）
- **广泛的生产力软件适配**：完美兼容 Microsoft OneNote, Photoshop, Clip Studio Paint, Krita, Drawboard PDF 等所有支持 Windows Ink 的绘图与笔记应用。

### 3. 多显示器 / 双屏精确投影映射
- 支持双显示器及多屏幕环境。
- iPad 端侧边栏可自由选择目标屏幕（`Display 1` / `Display 2` / `全部桌面`）。
- 自动适配 Windows 虚拟桌面坐标偏移，杜绝跨屏漂移或写出屏幕外的问题。

### 4. 优化 iPadOS Safari 体验
- **固定侧边栏**：侧边栏改为固定停靠，不会在左右划动写字时意外隐藏消失。
- **稳定连接**：修复 iPadOS Safari WebSocket 快速断连问题，支持静默自动重连。
- **手势防误触**：支持单指拖动平移视口、双指手势缩放，防止手掌触控产生多余划痕。

### 5. 全新 Windows 原生 GUI
- **纯净启动**：通过 Windows GUI 子系统运行，不再附带黑框命令行终端。
- **一键扫码**：启动自动开启服务，大尺寸局域网二维码即开即扫。
- **多样化 UI 主题**：内置 8 款视觉主题（Classic, Metro, Dark, Greybird 等），支持下拉即时切换并持久化保存。
- **可折叠彩色日志**：内置 ANSI 分级高亮日志监控（Debug 蓝 / Info 绿 / Warn 橙 / Error 红），支持一键清空折叠，折叠状态下彻底释放渲染资源。

---

## 🚀 快速上手 (Windows)

1. 前往 [Releases 页面](https://github.com/LycheeMiku/Weylus-lm39/releases) 下载最新版的预编译压缩包 `weylus-windows-x64-v1.0.1.zip`。
2. 解压并直接双击运行 `weylus.exe`。
3. 确保 iPad / 平板与电脑连接在**同一局域网**（Wi-Fi 或通过 USB 数据线共享网络）。
4. 在 iPad 上使用相机扫描客户端上的二维码，或在 Safari 中直接打开提示的地址（如 `http://192.168.x.x:1701`）。
5. **推荐技巧**：在 iPad Safari 中点击右上角的「分享」按钮 -> **「添加到主屏幕」**，之后从桌面启动，即可享受全屏沉浸式的数位板体验！

---

## 🛠️ 从源码编译 (Build from Source)

### 环境要求
- Rust 1.75 或更高版本
- Windows 10 / 11 64位系统
- C/C++ 编译工具链（Visual Studio Build Tools 或 MSVC）

### 编译步骤
```bash
# 克隆仓库
git clone https://github.com/LycheeMiku/Weylus-lm39.git
cd Weylus-lm39

# 构建 Release 可执行程序
cargo build --release

# 编译好的程序位于 target/release/weylus.exe
```

---

## 🔗 相关项目与致谢

- 原项目：[H-M-H/Weylus](https://github.com/H-M-H/Weylus)（感谢原作者 HMH 搭建的优秀跨平台基座）
- 本项目 Fork：[LycheeMiku/Weylus-lm39](https://github.com/LycheeMiku/Weylus-lm39)

## 📄 开源许可证

本项目遵循 [GNU Affero General Public License v3.0 (AGPL-3.0)](LICENSE)。
