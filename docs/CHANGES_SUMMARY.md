# Moonlight Android 修改總結

## 1. 移除 H.264 和 720P 支持

### 目的
專注於高質量串流體驗，移除較舊的編碼格式和低分辨率選項。

### 修改內容

#### 視頻格式
- **移除 H.264 (AVC) 支持**:
  - 從 `FormatOption` 枚舉中移除 `FORCE_H264` 選項
  - 從視頻格式選擇數組中移除 H.264 選項
  - 更新 `Game.java` 中的支持格式檢測，改為以 HEVC 為基礎
  - 移除 `DecoderCapabilityChecker` 中的 H.264 強制檢查
  - 默認支持格式從 `VIDEO_FORMAT_H264` 改為 `VIDEO_FORMAT_H265`

#### 分辨率
- **移除 720P (1280x720) 支持**:
  - 從 `arrays.xml` 分辨率列表中移除 720P 選項
  - 從 `PreferenceConfiguration` 中移除 `RES_720P` 常量
  - 更新 `isNativeResolution()` 方法，移除 720P 檢查
  - 默認分辨率從 720P 升級到 1080P
  - 更新 `StreamConfiguration` 默認寬高為 1920x1080

#### 向後兼容
- **自動升級舊設置**:
  - `convertFromLegacyResolutionString()` 將 720P 映射到 1080P
  - `readPreferences()` 中的舊版本 720P 設置自動升級為 1080P
  - 比特率計算表中移除 720P 條目，重新調整插值

### 影響的文件
1. **arrays.xml**: 移除 720P 和 H.264 選項
2. **PreferenceConfiguration.java**: 更新默認值和枚舉
3. **StreamConfiguration.java**: 更新默認分辨率和視頻格式
4. **Game.java**: 移除 H.264 支持檢測
5. **DecoderCapabilityChecker.java**: 移除 FORCE_H264 檢查

### 效果
- 所有串流現在至少使用 1080P 分辨率
- 僅支持 HEVC (H.265) 和 AV1 編碼
- 舊設備上配置的 720P 會自動升級到 1080P
- 提升整體串流質量和性能
- 簡化設置選項，減少用戶混淆

## 2. USB 權限持續授權與 Razer Kishi 系列支持

### 問題
每次連接 USB 遊戲控制器時，Android 都會要求重新授予 USB 權限。這會打斷遊戲體驗。

### 修復內容
- **usb_device_filter.xml** (新建):
  - 創建 USB 設備過濾器，聲明支持的所有 Xbox 控制器（Xbox One、Xbox 360、Xbox 360 無線）
  - 包含所有支持的廠商 ID（Microsoft、Mad Catz、Razer、PowerA、Hori 等）
  - **明確添加 Razer Kishi 系列支持**：Kishi、Kishi V2、Kishi V2 Pro、Kishi Ultra

- **AndroidManifest.xml**:
  - 在 `UsbDriverService` 中添加 `USB_DEVICE_ATTACHED` intent-filter
  - 添加 meta-data 指向 USB 設備過濾器資源
  - 這樣當支持的 USB 設備連接時，系統會自動授予權限

- **XboxOneController.java**:
  - 更新 Razer Vendor ID 註釋，明確列出支持的 Kishi 系列型號

- **Xbox360Controller.java**:
  - 更新 Razer Vendor ID 註釋，包含 Kishi 系列

### 支持的 Razer 控制器
- Razer Wildcat (Xbox One)
- Razer Kishi (原版，Android USB-C)
- Razer Kishi V2 (Android USB-C)
- Razer Kishi V2 Pro (Android USB-C)
- Razer Kishi Ultra (Android USB-C)
- Razer Sabertooth (Xbox 360)
- Razer Onza (Xbox 360，使用不同的 Vendor ID 0x1689)

### 效果
- 首次連接 USB 控制器時，系統會詢問是否允許應用使用該設備
- 勾選「默認使用此應用」後，以後連接相同設備不再需要重新授權
- 大幅提升用戶體驗，特別是頻繁連接/斷開控制器的場景
- **Razer Kishi 系列用戶現在可以享受無縫連接體驗**

## 2. 鍵盤輸入欄增強

### 功能
在鍵盤輸入欄中添加「發送並回車」按鈕，方便在遊戲中快速發送命令。

### 實現內容
- **activity_game.xml**:
  - 添加 `keyboardInputSendEnter` 按鈕

- **strings.xml**:
  - 添加 `keyboard_input_send_enter` 字符串資源

- **Game.java**:
  - 添加「發送並回車」按鈕的點擊處理程序
  - 添加 `sendEnterKey()` 方法發送 Enter 鍵事件
  - 添加 `sendEnterKeyDelayed()` 方法，延遲 100ms 發送 Enter 鍵，確保文字先被處理

### 按鈕說明
- **Send** - 僅發送文字
- **Send + Enter** - 發送文字並自動按下回車鍵
- **Cancel** - 關閉輸入欄

## 3. Pairing 實現錯誤修復

### 問題
在 `PairingService` 中創建 `AddressTuple` 時使用了錯誤的端口。應該使用 HTTP 端口（通常是 47989），而不是 HTTPS 端口（通常是 47984）。

### 修復內容
- **PairingService.java**:
  - 添加了 `EXTRA_COMPUTER_HTTP_PORT` 常量
  - 更新 `doPairing` 方法簽名，添加 `httpPort` 參數
  - 修正 `AddressTuple` 創建邏輯，使用正確的 HTTP 端口

- **PcView.java**:
  - 在啟動 PairingService 時傳遞 `computer.activeAddress.port`（HTTP 端口）

## 4. 通知權限正確請求（Android 13+）

### 問題
在 Android 13 (API 33+) 上，`POST_NOTIFICATIONS` 權限需要在運行時請求。配對服務啟動前台服務時沒有請求此權限。

### 修復內容
- **PcView.java**:
  - 添加 `REQUEST_NOTIFICATION_PERMISSION` 常量
  - 添加 `pendingPairComputer` 字段保存待配對的電腦
  - 在 `doPair` 方法中檢查通知權限
  - 添加 `onRequestPermissionsResult` 處理權限請求結果
  - 將配對服務啟動邏輯提取到 `startPairingService` 方法

## 5. 配對時自動複製 PIN 並打開瀏覽器

### 功能
配對開始時：
1. 自動將 PIN 碼複製到剪貼板
2. 自動打開瀏覽器到服務器的 Web 界面（HTTPS 端口）

### 實現內容
- **PairingService.java**:
  - 在 `onStartCommand` 中添加自動複製 PIN 到剪貼板的邏輯
  - 添加 `openPairingWebPage` 方法，打開瀏覽器到服務器的 HTTPS 地址
  - 支持 IPv6 地址格式化

- **strings.xml**:
  - 添加 `pair_browser_open_failed` 字符串資源

## 6. 簡化添加服務器為對話框

### 問題
原來使用單獨的 Activity (`AddComputerManually`) 來添加服務器，用戶體驗不夠流暢。

### 修復內容
- **Dialog.java**:
  - 添加 `AddComputerCallback` 接口
  - 添加 `displayAddComputerDialog` 方法，顯示輸入對話框
  - 對話框包含輸入框、確定和取消按鈕
  - 支持 IME 操作（Enter 鍵提交）

- **PcView.java**:
  - 修改添加服務器按鈕點擊事件，調用對話框而不是啟動 Activity
  - 添加 `showAddComputerDialog` 方法處理對話框邏輯
  - 添加 `parseHostInput` 方法解析用戶輸入（支持 IPv4、IPv6、主機名和端口）

## 5. 簡化性能統計通知內容

### 功能
- 簡化性能統計通知顯示的內容
- 只顯示關鍵指標：FPS、Bitrate、RTT（延遲）
- 格式：`60.00 FPS | 15.20 Mbps | 2 ms`

### 實現內容
- **StatsNotificationHelper.java**:
  - 添加 `simplifyStatsText` 方法解析和簡化統計文本
  - 使用 `lastIndexOf` 正確查找數值的起始位置
  - 從完整統計文本中提取 FPS、Mbps 和 RTT
  - 移除了 BigTextStyle，使用更緊湊的格式

## 6. 小米 HyperOS 3 超級島支持

### 功能
在小米 HyperOS 3+ 設備上，統計通知將使用超級島（Capsule）模式顯示。

### 實現內容
- **StatsNotificationHelper.java**:
  - 添加 `detectXiaomiHyperOS` 方法檢測小米設備
  - 添加 `getHyperOSVersion` 方法獲取 HyperOS 版本
  - 在通知中添加小米超級島相關的 extras：
    - `miui.extra.notification.use_capsule`: true
    - `miui.extra.notification.capsule_style`: 1 (緊湊模式)

### 參考文檔
- https://dev.mi.com/xiaomihyperos/documentation/detail?pId=2140

## 7. 添加 WakeLock 防止設備休眠

### 功能
在串流期間保持設備屏幕常亮，防止設備進入休眠狀態。

### 實現內容
- **Game.java**:
  - 添加 `PowerManager.WakeLock` 字段
  - 在 `onCreate` 中獲取 `SCREEN_BRIGHT_WAKE_LOCK | ACQUIRE_CAUSES_WAKEUP` 類型的 WakeLock
  - 在串流開始時獲取 WakeLock
  - 在 `onDestroy` 中釋放 WakeLock
  - 使用 `setReferenceCounted(false)` 避免引用計數問題

### 注意事項
- 使用 `SCREEN_BRIGHT_WAKE_LOCK` 保持屏幕最亮狀態
- 使用 `ACQUIRE_CAUSES_WAKEUP` 在獲取鎖時喚醒設備
- 已在 AndroidManifest.xml 中聲明 `WAKE_LOCK` 權限

## 8. 在性能統計通知中顯示視頻編碼器

### 功能
在性能統計通知標題中顯示當前使用的視頻編碼器（H.264、HEVC、AV1 等）。

### 實現內容
- **Game.java**:
  - 添加 `activeVideoCodec` 字段保存當前編碼器名稱
  - 添加 `getVideoCodecName` 方法將視頻格式代碼轉換為編碼器名稱
    - 支持 H.264
    - 支持 HEVC / HEVC Main10
    - 支持 AV1 Main8 / AV1 Main10
  - 在 `onPerfUpdate` 中更新 `activeVideoCodec` 並傳遞給 StatsNotificationHelper

- **StatsNotificationHelper.java**:
  - 更新 `showNotification` 方法接收編碼器名稱參數
  - 在通知標題中顯示編碼器信息
  - 格式：`H.264 - Moonlight Streaming` 或 `HEVC Main10 - Moonlight Streaming`

### 編碼器識別
- **H.264**: 基礎編碼器，所有設備支持
- **HEVC**: H.265 編碼器
- **HEVC Main10**: H.265 10-bit HDR 編碼器
- **AV1**: AV1 8-bit 編碼器
- **AV1 Main10**: AV1 10-bit HDR 編碼器

## 修改的文件列表

1. `app/src/main/java/com/limelight/computers/PairingService.java`
2. `app/src/main/java/com/limelight/PcView.java`
3. `app/src/main/java/com/limelight/utils/Dialog.java`
4. `app/src/main/java/com/limelight/utils/StatsNotificationHelper.java`
5. `app/src/main/java/com/limelight/Game.java` ⭐ 新增
6. `app/src/main/res/values/strings.xml`

## 測試建議

### 配對功能測試
1. 在 Android 13+ 設備上測試配對功能
2. 驗證通知權限請求是否正常彈出
3. 驗證 PIN 碼是否自動複製到剪貼板
4. 驗證瀏覽器是否自動打開到服務器 Web 界面
5. 測試 IPv4 和 IPv6 服務器地址

### 添加服務器測試
1. 測試對話框輸入各種格式的地址（IP、域名、帶端口）
2. 測試空輸入的錯誤提示
3. 測試取消操作

### 性能統計測試
1. 啟用性能統計通知
2. 驗證通知內容是否簡化（只顯示 FPS、Bitrate、RTT）
3. 驗證編碼器名稱是否正確顯示在通知標題中
4. 測試不同視頻格式（H.264、HEVC、AV1）的顯示
5. 在小米 HyperOS 3+ 設備上驗證超級島顯示效果
6. 在非小米設備上驗證正常通知顯示

### WakeLock 測試
1. 開始串流後，鎖定設備，驗證屏幕是否保持常亮
2. 長時間串流（超過設備默認休眠時間），驗證設備是否不會休眠
3. 退出串流後，驗證 WakeLock 是否正確釋放
4. 驗證電池消耗是否在合理範圍內

## 已知限制

1. **HyperOS 檢測**：使用反射訪問系統屬性，某些設備可能無法檢測
2. **超級島支持**：依賴小米的私有 API，未來版本可能變化
3. **統計文本解析**：基於當前格式，如果格式變化需要更新解析邏輯
4. **WakeLock 電池消耗**：保持屏幕常亮會增加電池消耗，但這是串流應用的必要功能

## 性能影響

- **WakeLock**: 輕微增加電池消耗（保持屏幕常亮）
- **編碼器檢測**: 每次性能更新時調用一次，性能影響可忽略
- **統計文本解析**: 字符串操作，性能影響可忽略
- **HyperOS 檢測**: 僅在初始化時執行一次，無運行時影響

