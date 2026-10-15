<img src="assets/app-icon.svg" width="96" alt="flytoSnap" />

# flytoSnap

简体中文 | [繁體中文](README.zh-TW.md) | [English](README.en.md)

原生 macOS 截图和录屏工具, 使用 Rust 实现. 按一个快捷键, 即可在截图, 滚动截图, 录屏和提取文字之间切换.

支持 macOS 14+, 界面支持简体中文, 繁体中文和英语, 默认跟随系统语言. 启动后驻留菜单栏, 不自动打开主窗口.

![截图, 遮罩, 工具栏和悬停提示](docs/images/screenshot-zh-CN.png)

## 功能

- 截图: 自动识别窗口, 自由框选, 放大镜, 八个调整点. 选区外变暗, 光标随按钮, 边缘, 角点和文字区域变化.
- 标注: 矩形, 椭圆, 箭头, 画笔, 高亮, 两种马赛克, 文字, 气泡标签和水印. 支持撤销, 重做和重新编辑.
- 贴图: 置顶, 拖动, 缩放, 透明度, 锁定, 复制和保存.
- 滚动截图: 手动或自动滚动, 双向拼接, 回滚去重, 预览和继续标注.
- 录屏: 选择区域, MP4 或 GIF, 可选择显示鼠标. 停止后可预览, 保存或复制到支持文件的应用中粘贴.
- 提取文字: 使用本机 Apple Vision, 结果可编辑和复制. 无需额外下载模型.

![MP4 和 GIF 录屏设置](docs/images/recording-zh-CN.png)

以上为应用原生控件的离屏截图, 内容来自内置演示数据.

## 构建和使用

需要 Rust 工具链和 macOS SDK.

```sh
bash scripts/build-app.sh
open dist/flytoSnap.app
```

构建脚本生成本地签名的 `.app`, 应用图标和 ZIP 测试包. 首次截图或录屏需要在 macOS 中允许屏幕录制权限, 授权后可能需要重新打开应用. 自动滚动另需辅助功能权限.

默认入口为 `Command+Shift+A`. 截图界面顶部可选择四种模式, 每个工具都有悬停提示. 在菜单栏的设置中可以更改快捷键, 保存目录和界面语言.

| 操作 | 快捷键 |
| --- | --- |
| 呼出截图和模式选择 | `Command+Shift+A` |
| 复制截图 | `Enter` 或 `Command+C` |
| 保存截图 | `Command+S` |
| 撤销 / 重做 | `Command+Z` / `Command+Shift+Z` |
| 退出截图 | `Escape` |
| 退出应用 | `Command+Q` |

贴图上滚动可缩放, `Option+滚动` 调整透明度, 右键打开操作菜单. 录屏的复制使用 macOS 文件剪贴板; GIF 同时提供动画数据, 粘贴效果取决于目标应用支持的格式. 录屏文件在本机缓存中保留, 复制后可以继续粘贴.

## 升级后权限无法生效

旧版测试包使用临时签名, 新版改为稳定的开发者证书签名. 这次签名身份切换需要重新授权. 应用提供打开系统设置, 显示当前应用和重新检测, 不自动重置系统权限.

1. 从菜单栏退出 flytoSnap. 将新版放到固定位置, 建议 `/Applications/flytoSnap.app`.
2. 打开系统设置 -> 隐私与安全性 -> 屏幕与系统音频录制 (旧系统显示屏幕录制).
3. 如果已有 flytoSnap 且开启开关仍无法截图, 移除旧条目, 通过 `+` 添加当前新版, 再打开开关.
4. 完全退出并重新打开应用. 必须替换权限列表中的旧条目, 仅打开旧条目的开关可能无效.

构建脚本优先使用本机 Developer ID Application 证书, 其次为 Apple Development 证书. 可用 `FLYTOSNAP_SIGNING_IDENTITY` 指定证书名称或指纹. 无有效证书时退回临时签名并显示警告, 此类包升级后可能需要重新授权. 后续构建请保持相同证书类型, 开发团队和应用标识. 构建脚本不改变钥匙串信任设置.

## 验证

```sh
cargo test
cargo clippy --all-targets -- -D warnings
cargo run -- --self-check target/self-check
cargo run -- --ui-snapshot target/ui-check --lang zh-CN
```

后两项不显示应用窗口. 检查覆盖编辑几何, 光标区域, 遮罩, 语言文本, 原生控件, Retina 裁剪, 系统 OCR, GIF 动画和 H.264 MP4 封装.

当前录屏仅包含屏幕画面, 不录制声音或摄像头. 自动滚动拼接适合静态页面; 避免固定页头, 滚动条, 动画和视频. 长图上限为 8000 万像素和 32000 像素高. 本地测试包未做 Developer ID 公证, 正式分发需要另外签名和公证.

## 参考和许可证

交互参考 [飞书截图](https://www.feishu.cn/hc/zh-CN/articles/319286513754) 和 [滚动截图](https://www.feishu.cn/hc/zh-CN/articles/145659580677). 原生接口使用 [objc2](https://github.com/madsmtm/objc2).

MIT, 见 [LICENSE](LICENSE). 图标沿用 FLYTO 蓝灰色, 橙色和对角框形品牌标记, 加入相机元素.
