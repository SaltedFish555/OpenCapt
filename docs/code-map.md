# OpenCapt 代码导图

English: [en/code-map.md](en/code-map.md)

## 先看哪里

如果你第一次阅读仓库，不建议直接扎进 `overlay` 细节。更高效的顺序是：

1. `src/main.rs`
2. `src/app.rs`
3. `src/config.rs` 和 `src/config/*`
4. `src/overlay.rs` 和 `src/overlay/*`
5. `src/settings.rs` 和 `src/settings/*`
6. `src/ocr/*`、`src/translation/*`
7. `src/pin.rs`

这样能先建立“系统是怎么跑起来的”，再进入截图与标注细节。

## 入口层

### `src/main.rs`

作用：

- 解析启动模式
- 加载配置与路径
- 初始化日志
- 分发到主程序、设置窗口或调试入口

当你想知道程序为什么会进入某个模式时，从这里看最合适。

### `src/app.rs`

作用：

- 持有主事件循环
- 协调托盘、热键、overlay、贴图和设置窗口
- 处理配置热重载

这是“运行时调度中心”。如果问题涉及托盘菜单、热键、配置变更后行为，优先看这里。

## 截图与标注层

### `src/overlay.rs`

这是 overlay 的总入口和 façade，负责：

- 导出公开类型
- 组织子模块
- 定义全局常量和高层结构

### `src/overlay/state.rs`

内部状态模型：

- 当前工具
- 当前选区
- 标注对象集合
- OCR/翻译显示状态
- 拖拽、调整尺寸、文字编辑等编辑态

### `src/overlay/input.rs`

输入处理：

- 鼠标事件
- 键盘事件
- 工具栏点击后的动作切换

如果你在改交互逻辑，通常先看这里。

### `src/overlay/render.rs`

主渲染与导出：

- 选区与暗层
- 标注对象渲染
- OCR/翻译覆盖层渲染
- 最终导出图片生成

### `src/overlay/draw.rs`

底层绘图原语：

- 像素级绘制
- GDI 文本栅格化
- `tiny-skia` 抗锯齿路径
- SVG 图标贴图

如果视觉效果不对，但交互逻辑没问题，通常问题在这里。

### `src/overlay/text.rs`

文字工具专属逻辑：

- 文字框排版
- 光标与编辑状态
- 文本样式

### `src/overlay/toolbar.rs`

工具栏布局与按钮定义。

### `src/overlay/win32.rs`

overlay 窗口的原生 Win32 部分：

- 窗口类注册
- layered surface
- `wndproc`
- cursor 切换

## 设置与配置层

### `src/config.rs` + `src/config/*`

按职责拆为：

- `types.rs`：配置类型与默认值
- `compat.rs`：老配置兼容
- `paths.rs`：portable / appdata 路径选择
- `io.rs`：加载与写入

如果你要加配置字段，基本都要经过这里。

### `src/settings.rs` + `src/settings/*`

按页面和支持层拆开：

- `theme.rs`：视觉主题
- `profiles.rs`：模型相关文案和选项
- `pages/*`：每个设置页的绘制逻辑

设置页问题优先不要去 `app` 查，先看这里。

## Provider 层

### `src/ocr/*`

- `mod.rs`：统一入口
- `parse.rs`：解析 provider 响应
- `normalize.rs`：bbox 归一化
- `providers/*`：不同 OCR provider 的协议适配

### `src/translation/*`

- `mod.rs`：统一入口
- `parallel.rs`：块级并发翻译调度
- `parse.rs`：翻译响应解析
- `providers/*`：不同翻译 provider 的协议适配

## 其他支撑模块

- `src/capture.rs`：抓屏与 UI 元素探测
- `src/output.rs`：复制到剪贴板、保存 PNG
- `src/pin.rs`：贴图窗口
- `src/startup.rs`：开机自启同步
- `src/tray.rs`：托盘与菜单
- `src/hotkey.rs`：全局热键注册
- `src/memory.rs`：截图后工作集回收