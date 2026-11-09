<div align="center">

[English](README.md) · [Français](README.fr.md) · [Español](README.es.md) · **中文** · [العربية](README.ar.md) · [Русский](README.ru.md)

<a href="https://tahabenissa.github.io/winsploit/">
  <img src=".github/banner.svg" alt="Winsploit" width="720">
</a>

### 终端里的 Windows 原生安全工具箱。

**在一个键盘驱动的仪表盘中查找、安装并运行 45 款精选渗透测试和 CTF 工具。**<br>
无需 WSL，无需虚拟机，不必再猜哪些工具在 Windows 上真正可用。

<br>

<a href="https://tahabenissa.github.io/winsploit/"><img src="https://img.shields.io/badge/%E7%BD%91%E7%AB%99-89c2d2?style=for-the-badge&logo=googlechrome&logoColor=0c191d" alt="网站" height="36"></a>&nbsp;
<a href="https://tahabenissa.github.io/winsploit/docs/"><img src="https://img.shields.io/badge/%E6%96%87%E6%A1%A3-232831?style=for-the-badge&logo=readthedocs&logoColor=89c2d2" alt="文档" height="36"></a>&nbsp;
<a href="https://tahabenissa.github.io/winsploit/demo/"><img src="https://img.shields.io/badge/%E6%BC%94%E7%A4%BA-232831?style=for-the-badge&logo=gnubash&logoColor=89c2d2" alt="演示" height="36"></a>

<br>

[![CI](https://github.com/tahabenissa/winsploit/actions/workflows/ci.yml/badge.svg)](https://github.com/tahabenissa/winsploit/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/tahabenissa/winsploit?include_prereleases&sort=semver)](https://github.com/tahabenissa/winsploit/releases)
![platform](https://img.shields.io/badge/%E5%B9%B3%E5%8F%B0-Windows%2010%20%7C%2011-0078D6)
![tools](https://img.shields.io/badge/%E7%9B%AE%E5%BD%95-45%20%E6%AC%BE%E5%B7%A5%E5%85%B7-8A2BE2)
![license](https://img.shields.io/badge/%E8%AE%B8%E5%8F%AF%E8%AF%81-MIT-green)

</div>

---

> [!WARNING]
> **仅限授权使用。** Winsploit 用于获得授权的渗透测试、安全研究、CTF 和学习。只能对你拥有或已取得
> **明确书面许可** 的系统使用。未经授权的扫描或攻击在大多数法域都属违法，遵守你的交战规则是你自己的责任。

---

## 为什么选择 Winsploit？

Kali 是安全工具的大本营，但很多人在 **Windows** 上工作，而现有方案都要你做出妥协：

| 方案 | 优点 | 不足 |
|---|---|---|
| **Commando VM / FLARE-VM** | 出色、全面 | 一次性的 *虚拟机预配工具*，不是日常使用的启动器 |
| **PentestBox** | 思路正确：原生、便携 | 自 2016 年起停更，许可不清 |
| **UniGetUI** | 优秀的包管理 GUI | 没有安全方面的精选，也没有「运行工具」的流程 |
| **Winsploit** | 精选、原生、既能安装**又能**运行、MIT、持续开发 | Alpha 阶段 |

目前没有一个持续维护、许可清晰的 Windows 原生启动器能在同一界面里既 *安装* 又 *运行* 安全工具。
Winsploit 填补了这一空白，并且如实告诉你哪些在 Windows 上真正可用、哪些需要 Linux。

## 快速开始

**最简单（无需管理员）：** 从 [Releases](https://github.com/tahabenissa/winsploit/releases) 页面下载
`winsploit-<版本>-setup.exe`。安装程序会把它加入 PATH，可配置 Scoop、git、bucket 和 pipx，并让你勾选
工具包。打开一个**新的**终端，输入 `winsploit`。该二进制未签名（SmartScreen 会提示「更多信息」→「仍要
运行」）；请用 `SHA256SUMS` 校验。

**或从源码构建：**

**你需要：** Windows 10/11、[Scoop](https://scoop.sh) 和 [Rust 1.87+](https://rustup.rs)。

```powershell
# 1. 安装 Scoop（已有则跳过）
Set-ExecutionPolicy -ExecutionPolicy RemoteSigned -Scope CurrentUser
Invoke-RestMethod -Uri https://get.scoop.sh | Invoke-Expression

# 2. 构建 winsploit
git clone https://github.com/tahabenissa/winsploit
cd winsploit
cargo build --release

# 3. 把 `winsploit` 变成命令（将 target\release 加入用户 PATH，无需管理员）
.\target\release\winsploit setup --path --shortcut

# 4. 打开新终端，进入你的工作目录并启动
cd C:\ctf\some-box
winsploit
```

用 `↑/↓` 选择工具（或用 `/` 过滤），按 `i` 安装、按 `Enter` 运行。任何时候按 `?` 查看全部按键。

## 功能

### 为键盘而生的仪表盘
- **按类别浏览**，实时显示安装状态：`●` 已安装、`○` 缺失、`◐` 冲突。
- **即时过滤（`/`）**：按名称、类别或描述。
- 参数输入是**真正的行编辑器**，并带有每个工具的**历史记录**（`↑/↓`）。
- **按键帮助（`?`）**。

### 实时、可交互的输出
- stdout/stderr **实时流入输出面板**，带颜色区分。
- **与运行中的工具对话。** 每个进程都有自己的输入管道（`Ctrl+D` = 输入结束）。
- **回滚**（`PgUp/PgDn/Home/End`）与**输出内查找**（`f`、`n`、`N`）。
- **`c` 取消**整个进程树，而不只是父进程。

### 一键安装，做到位
- 原生工具用 **Scoop**，首次需要时**自动添加缺失的 bucket**。
- Python 工具用 **pipx**（各自独立 venv；缺少时先装 Python 和 pipx）。
- **Winsploit 自有的 Scoop bucket** 收录无人打包的工具，带 **SHA-256 校验**。
- 检测在 **UI 线程之外**运行，仪表盘永不卡死。

### 为 CTF 打造
- **题目工作区（`C`）**：每道题一个目录，含目标、flag 格式、文件、笔记、日志和 flag。
- **配方**：在 nmap、ffuf、john 等上按 `Enter`，提供已填好 `{target}`、`{file}`、`{out}`、`{wordlist}` 的现成命令。
- **Flag 捕获器**：`picoCTF{…}`、`HTB{…}`、`flag{…}` 会被高亮、提示、复制并保存。

### 回路中的 AI 智能体
- 按 **`a`**：AI 读取当前终端的输出，并以**你确认后才会运行的「目录内工具」命令**形式给出下一步建议。
  智能体自身绝不运行 shell。
- **`A`** 选择提供方：**Claude CLI**（复用你的登录，无需密钥）、**Gemini / DeepSeek / OpenAI**，或本地
  **Ollama** 模型——每个提供方可单独设置模型。密钥从环境变量读取。

### 浏览器中的同一会话
终端仍是主程序，浏览器是指向**同一实时会话**的第二个窗口。`w` 启动/停止 Web 视图，`o` 打开它。
**`--lan`**（或 `W`）通过 **HTTPS** 把它提供给局域网，使用一次性访问密钥，且每位访客都需你批准。

### 配置驱动，设计上诚实
- 每个工具都是一条 **YAML 条目**，无需改代码即可添加。
- **冒名检测**：`verify:` 识别同名程序，拒绝启动错误的那个。

## 工具目录

**11 个类别共 45 款工具**，都因为在 Windows 上**原生**运行良好而入选。

| 类别 | 工具 |
|---|---|
| **侦察** | nmap · rustscan · gobuster · httpx · subfinder · amass · gau |
| **Web** | sqlmap · nuclei · ffuf · feroxbuster · katana · arjun · dirsearch · wfuzz · commix · nikto · ZAP (GUI) |
| **口令** | hashcat · John the Ripper (jumbo) · hashid · NetExec |
| **密码学** | openssl · RsaCtfTool · CyberChef (浏览器) |
| **取证** | exiftool · strings (Sysinternals) · Sleuth Kit · Volatility 3 |
| **逆向** | radare2 · upx · jadx · apktool · Detect It Easy · Cutter (GUI) · x64dbg (GUI) · dnSpy (GUI) · Ghidra (GUI) |
| **Pwn** | pwntools · ropper |
| **OSINT** | theHarvester · Sherlock |
| **隐写** | steghide |
| **抓包** | tshark |
| **字典** | SecLists-lite（数据） |

> [!IMPORTANT]
> **关于无线，说实话：** 在 Windows 上用普通网卡，Wi-Fi 监听模式抓包和注入**无法工作**。这里只有对已
> 抓获文件的*离线*破解有用。要做真正的无线测试，请用 Linux 配支持的 USB 网卡。

## 用法

```text
winsploit                 启动交互式仪表盘
winsploit list            列出工具目录及安装状态
winsploit hashid <h>…     识别一个或多个哈希（给出 hashcat -m / john --format 提示）
winsploit --help, -h      显示帮助
winsploit --version, -V   显示版本
```

### 仪表盘按键

| 按键 | 动作 |
|---|---|
| `↑` `↓` | 移动选择 |
| `/` | 按名称、类别或描述过滤 |
| `Enter` | 运行所选工具（先编辑预填参数，再按 `Enter`） |
| `i` | 安装所选工具（Scoop 或 pipx） |
| `r` | 重新检测已安装项 |
| `f` · `n` · `N` | 在输出中查找 · 上一个 · 下一个 |
| `c` | 取消正在运行的进程（整个进程树） |
| `h` | 识别哈希（内置、离线） |
| `a` · `A` | AI 智能体：读取输出并建议下一步 · 选择提供方 / 模型 |
| `p` | 项目：创建 / 打开工作目录，含最近项 |
| `d` | 工作目录：工具运行所在，像 `cd` |
| `C` | 题目：创建、切换、设置目标 / flag 格式 |
| `F` | 从输出中捕获的 flag |
| `S` | 在 Windows Sandbox 中运行所选工具 |
| `t` | 外观：主题、边框、字形（实时预览） |
| `w` · `W` · `o` | Web 视图：启动 / 提供给局域网 / 打开 |
| `?` | 显示全部按键 |
| `q` | 退出 |

---

📖 **完整参考**（安全模型、架构、添加工具、配方、沙箱等）：请见作为权威版本的
**[英文 README](README.md)**。

## 贡献

添加工具是最容易的起点：在 [`tools.yaml`](tools.yaml) 里加一条（如无人打包，再加一个 `bucket/`
清单），运行 `cargo test`，然后提交 PR。请只添加在 Windows 上真正能原生运行的工具，并用 `note:`
记录注意事项。

## 许可证

[MIT](LICENSE) © Winsploit 贡献者。每个随附或安装的工具保留其各自的许可证。
