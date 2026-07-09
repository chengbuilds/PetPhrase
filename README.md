<div align="center">

# 🐾 PetPhrase — Windows 桌宠常用语与剪贴板短语管理器

**一只桌宠 + 你的常用语面板**

桌宠静坐桌面,点它弹出常用语面板,点一条短语立即复制到剪贴板。<br>
客服话术、快捷回复、报销模板、代码片段——一次点击,不打断手头工作。

<sub>**English** · PetPhrase is a lightweight desktop pet + phrase manager for Windows: a petdex-style pet sits on your desktop — click it to pop up your phrase panel, click a phrase to copy it to the clipboard. Great for canned responses, quick replies, email templates and code snippets. Single-process native app built with Rust + Slint.</sub>

[![Release](https://img.shields.io/github/v/release/chengbuilds/PetPhrase?color=f97316&label=release)](https://github.com/chengbuilds/PetPhrase/releases/latest)
[![Downloads](https://img.shields.io/github/downloads/chengbuilds/PetPhrase/total?color=blue&label=downloads)](https://github.com/chengbuilds/PetPhrase/releases)
[![Platform](https://img.shields.io/badge/platform-Windows%2010%2F11%20x64-0078d6?logo=windows)](https://github.com/chengbuilds/PetPhrase/releases/latest)
[![Built with Slint](https://img.shields.io/badge/built%20with-Rust%20%2B%20Slint-orange?logo=rust)](https://slint.dev/)
[![Last Commit](https://img.shields.io/github/last-commit/chengbuilds/PetPhrase?color=green)](https://github.com/chengbuilds/PetPhrase/commits/main)
[![License](https://img.shields.io/github/license/chengbuilds/PetPhrase?color=success)](LICENSE)
[![Stars](https://img.shields.io/github/stars/chengbuilds/PetPhrase?style=social)](https://github.com/chengbuilds/PetPhrase/stargazers)

**单进程原生应用:安装包不到 10MB · 常驻内存约为 WebView 同类方案的 1/10 · 冷启动秒开**

</div>

---

## ✨ 它长什么样

| 桌宠静坐桌面 | 点它,面板贴宠弹出 | 挑选桌宠、调大小、换主题 | 管理分组与常用语 |
|:---:|:---:|:---:|:---:|
| <img src="screenshots/pet-desktop.png" height="180" alt="Windows 桌宠悬浮在桌面上(desktop pet)"> | <img src="screenshots/panel.png" height="180" alt="点击桌宠弹出的常用语快捷复制面板(phrase panel)"> | <img src="screenshots/settings-pets.png" height="180" alt="桌宠选择、大小与主题外观设置"> | <img src="screenshots/settings-phrases.png" height="180" alt="常用语分组与短语管理设置"> |

## 🚀 快速上手

1. 从 [Releases](https://github.com/chengbuilds/PetPhrase/releases/latest) 下载 `PetPhrase_x.y.z_x64-setup.exe`,双击安装(装到用户目录,**无需管理员权限**)

   > ⚠️ **首次运行会出现 Windows 风险提示**:安装包未购买代码签名证书,SmartScreen 会弹「Windows 已保护你的电脑」。点 **「更多信息」→「仍要运行」** 即可。代码全部开源,安装包由本仓库源码构建,可自行审计或[从源码构建](#-从源码构建)。
2. 桌面右下角出现一只桌宠——**点击它**,弹出常用语面板
3. 点右上角 ⚙ 打开设置,建分组、加常用语(支持多行文本)
4. 之后每次点桌宠 → 点短语 → 已复制 ✓,直接去粘贴

## 🎯 功能

- **桌宠**:透明置顶、雪碧图待机动画,点击招手,可拖拽并记住位置;大小三档可调(小/中/大),缩放时脚位不动
- **常用语面板**:贴宠弹出;分组胶囊 Tab + 全组宫格;短句排成气泡流、长句卡片;搜索跨组过滤;点击复制 + ✓ 反馈 + 自动收起;失焦即隐
- **📌 面板常驻**:点亮图钉后连续复制多条不收起,拖动桌宠面板实时跟随;再点图钉/点桌宠/Esc 收起
- **右键就地管理**:面板里右键短语可直接编辑/删除,右键空白处快速添加,弹出即可输入,不用打开设置窗
- **分组管理**:增删改、长按拖动排序、12 个分组图标
- **主题**:亚克力磨砂 / 实底浅色两种面板主题
- **数据**:开机自启可选;JSON 导入导出;数据原子写入 + 启动备份自动恢复,卸载重装不丢
- **一键更新**:自动/手动检查新版本(可关),一键下载校验并静默升级,装完自动回到桌面
- **标准安装/卸载**:NSIS 安装包,含 `uninstall.exe` 与「应用和功能」卸载入口,卸载时可选保留数据

## 🐱 换一只喜欢的桌宠

PetPhrase 兼容 [petdex](https://petdex.dev/) 桌宠格式——那个在 X 上火过的 Codex 桌宠生态。

**前置条件**:已安装 [Node.js](https://nodejs.org/)(自带 `npx`,无需全局安装 petdex)。

去 [petdex.dev](https://petdex.dev/) 逛一圈,挑一只喜欢的,一行命令装好:

```bash
npx petdex@latest install <pet-name>

# 例如,装爱坤和哆啦A梦:
npx petdex@latest install kun-like
npx petdex@latest install doraemon
```

| `kun-like` | `doraemon` |
|:---:|:---:|
| ![爱坤](screenshots/kun-like.png) | ![哆啦A梦](screenshots/doraemon.png) |

然后打开 PetPhrase 设置 →「外观与行为」,新桌宠已经出现在列表里,点选即换,**无需重启**。同页还能调桌宠大小(小/中/大)。

> 常用命令的话,也可以 `npm install -g petdex`,之后直接敲 `petdex install <pet-name>`。

也支持完全自定义:任意目录放上 `pet.json + spritesheet.webp/png`(参考 [petdex 格式](https://github.com/crafter-station/petdex)),在设置里指定该目录即可。

## 📋 使用示例

给客服 / 运营 / 开发同学的分组灵感:

| 分组 | 示例短语 |
|------|---------|
| 💬 客服 | 「您好,感谢您的反馈,我们已经记录了这个问题,会尽快给您答复」 |
| 📧 邮件 | 「收到,马上处理」「好的,没问题」 |
| 💻 代码 | `git rebase -i HEAD~3`、常用正则、SQL 片段 |
| 💳 账号 | 发票抬头、收货地址、公司税号 |

短句(≤10 字)自动排成紧凑气泡流,长文本独占卡片,一眼扫到。

## 🖥️ 平台支持

**目前仅支持 Windows 10/11 x64**,暂无 macOS / Linux 版本。

好消息是:UI 框架 [Slint](https://slint.dev/) 本身跨平台,核心逻辑(存储/布局/动画)与平台无关,移植的主要工作量在窗口特效(亚克力/点击穿透)、托盘、开机自启的平台适配。**欢迎 macOS / Linux 用户提 PR!** 提前开 [Issue](https://github.com/chengbuilds/PetPhrase/issues) 讨论方案更好。

## 🔧 从源码构建

需要 Rust stable 工具链(Windows MSVC)。

```bash
cd app-slint
cargo test                # 单元测试
cargo build --release     # 产出 target/release/PetPhrase.exe
# 打包安装程序(需 NSIS makensis):
makensis installer.nsi    # 产出 target/PetPhrase_<版本>_x64-setup.exe
```

数据文件位于 `%APPDATA%\PetPhrase\{phrases.json, settings.json}`。

## 🤝 贡献

- 🐛 **报 Bug / 提需求**:[Issues](https://github.com/chengbuilds/PetPhrase/issues) 欢迎任何反馈,中文英文都行
- 🔀 **提 PR**:改动前跑一遍 `cargo test`;涉及 UI 手感的改动建议附截图/录屏
- 🍎🐧 **平台移植**:macOS / Linux 适配是最受欢迎的贡献方向
- ⭐ **觉得好用?** 点个 Star 是最直接的支持

## 🙏 致谢

- 桌宠格式与生态来自 [petdex](https://petdex.dev/)([crafter-station/petdex](https://github.com/crafter-station/petdex))
- UI 框架 [Slint](https://slint.dev/) · 图标 [Lucide](https://lucide.dev/)

## 📄 许可

[MIT](LICENSE) © 2026 chengbuilds
