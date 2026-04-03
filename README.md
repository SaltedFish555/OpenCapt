# OpenCapt

OpenCapt 是一个用 Rust 编写的 Windows 截图工具 MVP，目标是先做出类似 Snipaste 的核心闭环：

- 托盘常驻
- 全局热键唤起截图
- 鼠标框选区域
- 自动复制到剪贴板
- 自动保存 PNG

## 当前状态

当前仓库实现的是第一版 MVP：

- 默认热键 `Ctrl+Shift+A`
- 托盘菜单包含截图、打开截图目录、打开配置目录、退出
- 自动将截图保存到 `图片/OpenCapt/YYYY-MM-DD/`
- 自动在 `%APPDATA%/OpenCapt/config.toml` 生成默认配置

## 运行

确保当前终端能直接找到 Rust：

```powershell
$env:PATH="$env:USERPROFILE\.cargo\bin;$env:PATH"
cargo run
```

调试入口：

```powershell
cargo run -- capture-test
cargo run -- overlay-test
```

## 打包发布

一键打包（构建 release + 产出 zip）：

```powershell
Set-Location I:\MyCode\OpenCapt
.\build-release.ps1
```

可选参数：

```powershell
# 使用静态 CRT，减少目标机 VC 运行库依赖
.\build-release.ps1 -StaticCRT

# 只构建并导出目录，不压缩 zip
.\build-release.ps1 -SkipZip
```

默认输出目录：

```text
I:\MyCode\OpenCapt\dist\
```

## 配置

配置文件位置：

```text
%APPDATA%\OpenCapt\config.toml
```

默认配置：

```toml
hotkey = "Ctrl+Shift+A"
auto_copy = true
auto_save = true
save_dir = "C:\\Users\\<User>\\Pictures\\OpenCapt"
```

## 后续建议

- 基础标注
- 设置窗口
- 开机自启
- 贴图模式
