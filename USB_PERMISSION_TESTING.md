# USB 權限持續授權測試說明

## 測試目的
驗證 USB 遊戲控制器權限可以持續保存，不需要每次重新授權。

## 測試前提
- Android 設備已安裝 Moonlight
- 有支持的 USB 遊戲控制器（Xbox One、Xbox 360 或 Xbox 360 無線控制器）

## 測試步驟

### 第一次測試（首次連接）
1. 連接 USB 遊戲控制器到 Android 設備
2. 系統應該會彈出權限對話框：「允許應用 Moonlight 訪問 USB 設備？」
3. **重要**：勾選「默認使用此應用程式」（Use by default for this USB device）
4. 點擊「確定」
5. 控制器應該可以正常工作

### 第二次測試（斷開重連）
1. 拔出 USB 控制器
2. 等待 2-3 秒
3. 重新插入 USB 控制器
4. **預期結果**：系統不應該再次詢問權限，控制器直接可用

### 第三次測試（應用重啟）
1. 完全關閉 Moonlight 應用
2. 拔出 USB 控制器
3. 重新打開 Moonlight 應用
4. 插入 USB 控制器
5. **預期結果**：系統不應該詢問權限，控制器直接可用

### 第四次測試（設備重啟）
1. 拔出 USB 控制器
2. 重啟 Android 設備
3. 設備啟動後，插入 USB 控制器
4. 打開 Moonlight 應用
5. **預期結果**：系統不應該詢問權限，控制器直接可用

## 支持的控制器

### Xbox One 系列
- Microsoft Xbox One 控制器
- Mad Catz Xbox One 控制器
- Razer Wildcat
- PowerA Xbox One 控制器
- Hori Xbox One 控制器
- Hyperkin Xbox One 控制器

### Xbox 360 系列
- Microsoft Xbox 360 有線控制器
- Microsoft Xbox 360 無線接收器
- Logitech Xbox 360 控制器
- Razer Sabertooth
- Razer Onza
- PowerA Xbox 360 控制器
- 8BitDo 控制器（Xbox 360 模式）
- 其他兼容控制器

## 技術實現說明

### 工作原理
1. 在 `res/xml/usb_device_filter.xml` 中聲明支持的 USB 設備（按 Vendor ID）
2. 在 `AndroidManifest.xml` 的 `UsbDriverService` 中添加 USB_DEVICE_ATTACHED intent-filter
3. 當用戶首次連接設備並選擇「默認使用此應用」時，Android 系統會記住這個選擇
4. 之後相同設備連接時，系統會自動授予權限給應用

### 注意事項
- 只有用戶在首次權限對話框中勾選了「默認使用此應用」，權限才會持續保存
- 如果用戶清除了應用數據或卸載重裝，需要重新授權一次
- 不同的 USB 設備（即使是相同型號）可能需要分別授權一次

## 故障排除

### 問題：仍然每次都要求權限
**可能原因**：
- 首次授權時沒有勾選「默認使用此應用」
- 控制器不在支持列表中

**解決方案**：
1. 在 Android 設置中清除 Moonlight 的默認值
2. 重新連接控制器
3. 確保勾選「默認使用此應用」選項

### 問題：控制器無法識別
**可能原因**：
- 控制器型號不在支持列表中
- Android 版本不支持該控制器

**解決方案**：
1. 檢查 `usb_device_filter.xml` 是否包含該控制器的 Vendor ID
2. 在設置中啟用「綁定所有 USB 設備」選項

## 相關文件
- `app/src/main/res/xml/usb_device_filter.xml` - USB 設備過濾器配置
- `app/src/main/AndroidManifest.xml` - UsbDriverService 配置
- `app/src/main/java/com/limelight/binding/input/driver/UsbDriverService.java` - USB 驅動服務

