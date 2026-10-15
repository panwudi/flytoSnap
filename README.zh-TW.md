<img src="assets/app-icon.svg" width="96" alt="flytoSnap" />

# flytoSnap

**[简体中文](README.zh-CN.md) | [繁體中文](README.zh-TW.md) | [English](README.en.md)**

[Home / 首页 / 首頁](README.md)

原生 macOS 截圖與錄影工具, 使用 Rust 實作. 按一個快捷鍵, 即可在截圖, 捲動截圖, 錄影和擷取文字之間切換.

支援 macOS 14+, 介面支援簡體中文, 繁體中文和英語, 預設跟隨系統語言. 啟動後駐留選單列, 不自動開啟主視窗.

![截圖, 遮罩, 工具列與滑鼠提示](docs/images/screenshot-zh-TW.png)

## 功能

- 截圖: 自動辨識視窗, 自由框選, 放大鏡與八個調整點. 選取區域外變暗, 游標隨按鈕, 邊緣, 角點和文字區域變化.
- 標註: 矩形, 橢圓, 箭頭, 畫筆, 螢光標記, 兩種馬賽克, 文字, 氣泡標籤和浮水印. 支援復原, 重做和重新編輯.
- 貼圖: 置頂, 拖曳, 縮放, 透明度, 鎖定, 複製和儲存.
- 捲動截圖: 選取區域後按一下開始捲動截圖, 再手動或自動捲動. 精簡控制列與側邊預覽保持可見, 支援雙向拼接, 往回捲動去重, 編輯, 儲存和複製.
- 錄影: 選擇區域, MP4 或 GIF, 可選擇顯示游標. 停止後可預覽, 儲存或複製至支援檔案的應用程式中貼上.
- 擷取文字: 使用本機 Apple Vision, 結果可編輯與複製. 不需另行下載模型.

![MP4 與 GIF 錄影設定](docs/images/recording-zh-TW.png)

選取區域後先確認開始, 再選擇自動捲動:

![捲動截圖開始確認](docs/images/scroll-ready-zh-TW.png)

![精簡捲動控制列](docs/images/scroll-controls-zh-TW.png)

以上為應用程式原生控制項的離屏截圖, 內容來自內建示範資料.

## 下載與自動更新

[下載最新 macOS 版本](https://github.com/panwudi/flytoSnap/releases/latest) (Apple Silicon, macOS 14+). 解壓縮後將 flytoSnap.app 放入應用程式資料夾.

從 0.3.0 起, 選單列提供檢查更新與自動更新開關. 預設每天檢查並下載簽署的更新, 結束應用程式時安裝; 截圖和錄影期間延後更新. 舊的 0.2.x 版本需先手動安裝一次.

## 建置與使用

需要 Rust 工具鏈和 macOS SDK.

```sh
bash scripts/build-app.sh
open dist/flytoSnap.app
```

建置腳本產生本機簽署的 `.app`, 應用程式圖示和 ZIP 測試包. 首次截圖或錄影需在 macOS 中允許螢幕錄製權限, 授權後可能需要重新開啟應用程式. 自動捲動另需輔助使用權限.

首次啟動時檢查並請求尚未授予的螢幕錄製權限; 已授權時不重複提示. 選單列提供取消入口. 普通截圖切換到其他應用程式時自動收起, 捲動截圖控制列保持可見. 截圖時按 `Escape`, 再按截圖快捷鍵或按一下取消即可離開. 再次開啟已執行的應用程式會顯示操作選單, 不開啟主視窗.

預設入口為 `Command+Shift+A`. 截圖介面上方可選擇四種模式, 每個工具都有滑鼠提示. 在選單列的設定中可以更改快捷鍵, 儲存資料夾與介面語言.

| 操作 | 快捷鍵 |
| --- | --- |
| 開啟截圖與模式選擇 | `Command+Shift+A` |
| 複製截圖 | `Enter` 或 `Command+C` |
| 儲存截圖 | `Command+S` |
| 復原 / 重做 | `Command+Z` / `Command+Shift+Z` |
| 離開截圖 | `Escape` |
| 結束應用程式 | `Command+Q` |

在貼圖上捲動可縮放, `Option+捲動` 調整透明度, 按右鍵開啟操作選單. 錄影的複製使用 macOS 檔案剪貼簿; GIF 同時提供動畫資料, 貼上效果取決於目標應用程式支援的格式. 錄影先產生暫存結果, 按一下儲存才匯出至所選位置. 關閉結果會清理未使用的暫存檔案; 已複製的錄影保留到剪貼簿更新, 結束應用程式後仍可貼上, 下次啟動時清理失效檔案.

## 升級後權限無法生效

舊版測試包使用臨時簽署, 新版改為穩定的開發者憑證簽署. 這次簽署身分切換需要重新授權. 應用程式提供開啟系統設定, 顯示目前的應用程式和重新檢查, 不自動重設系統權限.

1. 從選單列結束 flytoSnap. 將新版放到固定位置, 建議 `/Applications/flytoSnap.app`.
2. 開啟系統設定 -> 隱私權與安全性 -> 螢幕與系統音訊錄製 (舊系統顯示螢幕錄製).
3. 如果已有 flytoSnap 且開啟開關仍無法截圖, 移除舊項目, 透過 `+` 加入目前的新版, 再開啟開關.
4. 完全結束並重新開啟應用程式. 只開啟舊項目的開關可能無法授權新版.

建置腳本優先使用本機 Developer ID Application 憑證, 其次為 Apple Development 憑證. 可用 `FLYTOSNAP_SIGNING_IDENTITY` 指定憑證名稱或指紋. 沒有有效憑證時退回臨時簽署並顯示警告, 此類包升級後可能需要重新授權. 後續建置請保持相同憑證類型, 開發團隊和應用程式識別碼. 建置腳本不變更鑰匙圈信任設定.

## 驗證

```sh
cargo test
cargo clippy --all-targets -- -D warnings
cargo run -- --self-check target/self-check
cargo run -- --ui-snapshot target/ui-check --lang zh-TW
cargo run -- --window-check
```

後三項不在工作區顯示應用程式視窗. 檢查涵蓋編輯幾何, 游標區域, 遮罩, 語言文字, 原生控制項, Retina 裁剪, 系統 OCR, GIF 動畫與 H.264 MP4 封裝. `--window-check` 使用螢幕外的實際原生視窗, 檢查 WindowServer 層級, 遮擋偵測, 按鈕命中, 視窗事件分發, 錄影格式切換與取消. 此檢查不錄製螢幕或請求權限.

以下指令會短暫顯示實際選單列圖示約一秒, 不開啟主視窗或請求權限:

```sh
dist/flytoSnap.app/Contents/MacOS/flytosnap --menubar-check
```

目前錄影僅包含螢幕畫面, 不錄製聲音或攝影機. 自動捲動拼接適合靜態頁面; 避免固定頁首, 捲軸, 動畫和影片. 長圖上限為 8000 萬像素和 32000 像素高. 發行包使用 Developer ID 簽署與 Sparkle 更新簽章. Apple 公證狀態以 Release 說明為準.

## 參考與授權

互動參考 [飛書截圖](https://www.feishu.cn/hc/zh-CN/articles/319286513754) 和 [捲動截圖](https://www.feishu.cn/hc/zh-CN/articles/145659580677). 原生介面使用 [objc2](https://github.com/madsmtm/objc2).

MIT, 請見 [LICENSE](LICENSE). 圖示沿用 FLYTO 藍灰色, 橙色與對角框形品牌標記, 加入相機元素.
