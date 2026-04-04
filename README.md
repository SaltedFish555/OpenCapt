# OpenCapt

OpenCapt 是一个用 Rust 编写的 Windows 截图工具，当前目标是做出一套接近 Snipaste / PixPin 的桌面截图、标注、贴图与 OCR/翻译工作流。

当前项目是 `Windows only`，主程序常驻托盘，没有主窗口，截图与标注使用原生 overlay 渲染链路。

## 当前功能

### 截图

- 托盘常驻运行
- 全局热键唤起截图，默认 `Ctrl+Shift+A`
- 框选截图，`Esc` 取消
- 多显示器下按当前鼠标所在屏幕截图
- 非 100% 缩放下坐标对齐
- 自动复制到剪贴板
- 自动保存 PNG 到日期分目录

### 标注

- 选区移动
- 选区八个控制点缩放
- 鼠标 / 选择 / 矩形 / 椭圆 / 直线 / 箭头 / 马赛克 / 文字 / 序号
- 撤销
- 文字重新编辑
- 文字字体、字号、粗体、斜体、背景
- 标注对象按工具类型选择与编辑

### 贴图

- 截图后直接贴图
- 多张贴图并存
- 拖动、滚轮缩放
- 右键菜单
- 置顶开关
- 边框/阴影开关
- 不透明度调节

### OCR 与翻译

- OCR 覆盖层显示与点击复制
- 复制全部 OCR 文本
- OpenAI Compatible OCR
- 百度 OCR
- OpenAI Compatible 翻译
- 百度图片翻译
- 支持直接使用百度返回的译图
- 不同 OCR 模型支持不同 bbox 坐标范围

### 设置

- 独立设置窗口
- 通用 / 标注 / 贴图 / OCR / 翻译 五个页面
- 热键、保存目录、默认标注值、贴图默认值
- 开机自启开关
- OCR / 翻译模型管理与连接测试

## 运行开发版

确保当前终端可以直接找到 Rust：

```powershell
$env:PATH="$env:USERPROFILE\.cargo\bin;$env:PATH"
cargo run
```

调试入口：

```powershell
cargo run -- capture-test
cargo run -- overlay-test
cargo run -- settings
```

## 打包发布

一键打包：

```powershell
Set-Location I:\MyCode\OpenCapt
.\build-release.ps1
```

常用参数：

```powershell
# 使用静态 CRT，尽量减少目标机器对 VC 运行库的依赖
.\build-release.ps1 -StaticCRT

# 只生成目录，不压缩 zip
.\build-release.ps1 -SkipZip
```

默认输出目录：

```text
I:\MyCode\OpenCapt\dist\
```

release exe 已嵌入应用图标，图标来源是 `assets/icons/tray.ico`。

## 配置与目录

OpenCapt 现在优先使用便携式配置：

```text
opencapt.exe 同级目录\config.toml
opencapt.exe 同级目录\logs\
```

如果 exe 所在目录不可写，例如放在 `Program Files` 下，会自动回退到：

```text
%APPDATA%\OpenCapt\config.toml
%APPDATA%\OpenCapt\logs\
```

截图默认保存到：

```text
%USERPROFILE%\Pictures\OpenCapt\YYYY-MM-DD\
```

## 配置示例

```toml
[general]
hotkey = "Ctrl+Shift+A"
auto_copy = true
auto_save = true
launch_at_startup = false
save_dir = "C:\\Users\\<User>\\Pictures\\OpenCapt"

[annotation_defaults]
default_color_index = 4
stroke_width = 2
text_size = 24
number_size = 28
mosaic_size = 12
text_bold = false
text_italic = false
text_background = false
text_font_family = "yahei"

[pin_defaults]
always_on_top = true
show_decoration = true
opacity_percent = 100
```

OCR 和翻译模型配置可以直接通过设置窗口维护，通常不需要手改 `config.toml`。

## 技术栈

- `tao`：主事件循环
- `tray-icon`：系统托盘
- `global-hotkey`：全局热键
- `xcap`：屏幕截图
- 原生 Win32 layered window：overlay 与贴图
- `eframe/egui`：设置窗口
- `arboard`：剪贴板
- `serde + toml`：配置
- `reqwest`：OCR / 翻译接口调用

## 当前定位

当前项目已经不是最初的 MVP，而是一版可日常使用的 Windows 截图工具。后续如果继续迭代，更值得投入的方向通常是：

- 更强的文字排版与图片翻译回填
- 更完整的历史记录
- 安装包与代码签名
- 更细的 OCR / 翻译 provider 扩展
