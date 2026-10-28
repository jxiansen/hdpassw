# hdpassw（中文说明）

> 本分支在原作者 [hdpassw](https://github.com/whitepiratesmith/hdpassw) 基础上增加了
> **Windows 原生构建**和**中文界面支持**。协议仍为 BSD-2-Clause。

hdpassw 是一个确定性的密码管理器：只记住一组 **24 个英文单词的助记词**（BIP39 标准），
所有网站密码都由它现场派生，**不存储任何密码**。

```
密码 = f(助记词, 站点名, 用户名, 计数器)
```

同样的输入永远得到同样的输出，所以备份只需要一张写着 24 个单词的纸。

## 特性

- BIP39 助记词 + scrypt（128 MiB, N=2¹⁷）+ BLAKE3 派生，无自造密码学
- 密码不落盘；`sites.toml` 只含站点名/用户名等非机密元数据
- 每次生成后剪贴板 30 秒自动清空
- `bump` 引导式换密：先给旧密码登录，再给新密码去设置
- `pwned` 用 k-匿名查询 Have I Been Pwned（只上传 SHA-1 前 5 位）
- **图形界面（egui，纯 Rust 无 webview）**，CLI 与 GUI 共用同一份数据
- **中文 / English 双语界面**，右上角一键切换，选择持久保存
- **Windows 原生 exe**（Windows 10/11，x86_64）

## Windows 使用

下载/构建 `hdpassw-gui.exe` 双击运行，或使用 `hdpassw.exe` 命令行。

数据保存位置：`%APPDATA%\hdpassw\`

| 文件 | 内容 |
| --- | --- |
| `seed.vault` | 加密的助记词金库（需金库密码解锁） |
| `sites.toml` | 站点清单（不含密码） |
| `lang` | 界面语言偏好 |

### 从源码构建 Windows 版

**推荐：在 Windows 本机开发**（MSVC 工具链，产物体积更小、调试体验最好）

```powershell
# 一次性准备
rustup target add x86_64-pc-windows-msvc

# 构建（PowerShell 或 cmd）
cargo build --release --features gui --bins
# 产物：target\release\hdpassw.exe、target\release\hdpassw-gui.exe
```

本仓库的 `.cargo/config.toml` 已为 Windows 目标开启 **静态 CRT 链接**，
产物不依赖 `VCRUNTIME140.dll` 或 VC++ 运行库，拷到任意 64 位 Windows 都能直接运行。

**在 Linux 上交叉编译**（CI 采用这种方式，无需 Windows 机器）

```bash
# 方式 A：GNU 目标，需要 mingw-w64
sudo apt install gcc-mingw-w64-x86-64 binutils-mingw-w64-x86-64
./build-win.sh                  # 产物在 target/x86_64-pc-windows-gnu/release/

# 方式 B：MSVC 目标，用 cargo-xwin + LLVM（产物与 Windows 本机构建一致）
cargo install cargo-xwin
sudo apt install llvm
./build-win.sh --msvc           # 产物在 target/x86_64-pc-windows-msvc/release/
```

两种产物都只依赖 Windows 系统 DLL，无需附带任何运行时文件。

## Linux 使用

```bash
./build.sh --gui
sudo ./install.sh --gui
```

## 中文界面

- 自动检测：系统语言为中文（`LANG=zh*` 或 Windows 区域设置为中文）时默认中文
- 手动切换：窗口右上角「English / 中文」按钮，写入 `%APPDATA%\hdpassw\lang`
- 环境变量：`HDPASSW_LANG=zh` / `HDPASSW_LANG=en` 强制指定
- 字体：自动查找系统 CJK 字体（Windows 微软雅黑；Linux Noto/文泉驿）

## 测试

```bash
cargo test
```

## 发布流程（云端构建）

发布完全在 GitHub Actions 上进行，本地不需要编译：

1. 修改 `Cargo.toml` 版本号，更新 `CHANGELOG.md`
2. 可选：新增发布说明 `.github/releases/vX.Y.Z.md`
3. 提交并推送 `master`
4. 打标签并推送：

```bash
git tag -a vX.Y.Z -m "vX.Y.Z"
git push origin vX.Y.Z
```

推送 tag 后，Release 工作流会自动在云端完成：Linux 测试 → Linux/Windows 构建 →
打包（含校验和）→ 创建 GitHub Release 并上传资产。

如需手动触发：Actions → Release → Run workflow，填入 tag。

## 安全提醒

助记词是唯一秘密，丢失无法找回；不要截图、不要存网盘、不要发聊天软件。
金库密码遗忘同样无法恢复 `seed.vault`。
