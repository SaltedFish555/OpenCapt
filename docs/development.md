# 开发与调试说明

English: [en/development.md](en/development.md)

## 环境要求

- Windows 10 / 11
- Rust 工具链
- 当前终端能直接访问 `cargo` / `rustc`

如果终端里还找不到 Cargo，可临时这样处理：

```powershell
$env:PATH="$env:USERPROFILE\.cargo\bin;$env:PATH"
```

## 常用启动模式

普通启动：

```powershell
cargo run
```

抓屏测试：

```powershell
cargo run -- capture-test
```

直接打开截图 overlay：

```powershell
cargo run -- overlay-test
```

单独打开设置窗口：

```powershell
cargo run -- settings
```

## 配置与日志位置

OpenCapt 优先使用便携式路径：

```text
.\config.toml
.\logs\
```

如果当前 exe 目录不可写，则回退到：

```text
%APPDATA%\OpenCapt\config.toml
%APPDATA%\OpenCapt\logs\
```

截图默认保存到：

```text
%USERPROFILE%\Pictures\OpenCapt\
```

## 打包

```powershell
.\build-release.ps1
```

常用参数：

```powershell
.\build-release.ps1 -StaticCRT
.\build-release.ps1 -SkipZip
```

## 调试建议

### 改托盘 / 热键 / 配置热重载

优先看：

- `src/app.rs`
- `src/tray.rs`
- `src/hotkey.rs`
- `src/config/*`

### 改截图交互 / 选区 / 标注

优先看：

- `src/overlay/state.rs`
- `src/overlay/input.rs`
- `src/overlay/render.rs`
- `src/overlay/draw.rs`

### 改文字工具

优先看：

- `src/overlay/text.rs`
- `src/overlay/render.rs`

### 改 OCR / 翻译 provider

优先看：

- `src/ocr/*`
- `src/translation/*`

## 常见坑

### 1. 不要把设置窗口逻辑混进截图主链路

设置窗口是独立子系统；截图 overlay 追求的是原生交互，不适合直接迁移成普通 GUI 页面。

### 2. 改 OCR / 翻译时不要直接在 overlay 里写协议细节

协议适配应该留在 provider 层；overlay 只消费统一结果。

### 3. 关注高 DPI 与多显示器

截图工具最容易出问题的地方之一就是：

- 坐标错位
- 缩放不一致
- 首次截图缓存或内存行为异常

改动截图主链路后，应至少手测：

- 非 100% 缩放
- 多显示器
- 第一次截图和连续截图

### 4. 贴图窗口和截图 overlay 是两套原生窗口

它们都走 layered window，但职责不同。改一边的绘制逻辑时，不要默认另一边会自然同步。

## 推荐的本地验证

每次改动后至少做：

```powershell
cargo test
cargo build
```

如果改动涉及截图主链路，再补手测：

- 热键唤起截图
- 框选、取消、保存、复制
- 标注编辑
- OCR / 翻译
- 贴图