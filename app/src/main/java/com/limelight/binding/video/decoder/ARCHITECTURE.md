# MediaCodecDecoderRenderer 架構文檔

## 概述

`MediaCodecDecoderRenderer` 是 Moonlight Android 應用程序的核心視頻解碼組件，負責使用 Android MediaCodec API 解碼來自遊戲串流的視頻數據。

經過重構後，原本近 1900 行的單一類別被拆分為多個專注於單一職責的模塊。

## 架構圖

```
┌─────────────────────────────────────────────────────────────────┐
│                    MediaCodecDecoderRenderer                     │
│                      (主協調器 / Coordinator)                      │
├─────────────────────────────────────────────────────────────────┤
│  - 視頻解碼器生命週期管理                                           │
│  - 組件協調與整合                                                  │
│  - 公開 API 接口                                                   │
└─────────────────────────────────────────────────────────────────┘
                                    │
           ┌────────────────────────┼────────────────────────┐
           │                        │                        │
           ▼                        ▼                        ▼
┌─────────────────────┐  ┌─────────────────────┐  ┌─────────────────────┐
│ DecoderCapability   │  │ CodecRecovery       │  │ FrameRender         │
│ Checker             │  │ Manager             │  │ Controller          │
├─────────────────────┤  ├─────────────────────┤  ├─────────────────────┤
│ • 解碼器發現        │  │ • 異常處理          │  │ • 渲染線程管理      │
│ • 性能點檢測        │  │ • 恢復策略          │  │ • Choreographer     │
│ • 能力查詢          │  │ • 線程同步          │  │ • 輸出緩衝區隊列    │
└─────────────────────┘  └─────────────────────┘  └─────────────────────┘
           │                        │                        │
           │                        │                        │
           ▼                        ▼                        ▼
┌─────────────────────┐  ┌─────────────────────┐  ┌─────────────────────┐
│ CsdBuffer           │  │ Performance         │  │ DecoderExceptions   │
│ Processor           │  │ StatsManager        │  │                     │
├─────────────────────┤  ├─────────────────────┤  ├─────────────────────┤
│ • VPS/SPS/PPS 處理  │  │ • 幀統計收集        │  │ • DecoderHung       │
│ • H.264 SPS 修補    │  │ • 性能覆蓋層        │  │   Exception         │
│ • 比特流限制        │  │ • 延遲計算          │  │ • RendererException │
└─────────────────────┘  └─────────────────────┘  │ • RendererState     │
                                                   └─────────────────────┘
```

## 模塊詳細說明

### 1. DecoderCapabilityChecker

**檔案位置**: `decoder/DecoderCapabilityChecker.java`

**職責**:
- 發現可用的 AVC (H.264)、HEVC (H.265) 和 AV1 解碼器
- 檢查解碼器是否能滿足指定的性能點（分辨率 + 幀率）
- 確定參考幀失效 (RFI) 支持
- 計算最佳每幀切片數

**主要方法**:
```java
MediaCodecInfo getAvcDecoder()
MediaCodecInfo getHevcDecoder()
MediaCodecInfo getAv1Decoder()
boolean isHevcMain10Hdr10Supported()
boolean isAv1Main10Supported()
boolean decoderCanMeetPerformancePoint(VideoCapabilities caps, PreferenceConfiguration prefs)
```

### 2. CodecRecoveryManager

**檔案位置**: `decoder/CodecRecoveryManager.java`

**職責**:
- 管理編解碼器異常恢復
- 協調線程靜止以進行安全的編解碼器操作
- 實現漸進式恢復策略（刷新 → 重啟 → 重置 → 重建）
- 追蹤恢復嘗試次數

**恢復類型**:
| 類型 | 描述 |
|------|------|
| `RECOVERY_TYPE_NONE` | 無需恢復 |
| `RECOVERY_TYPE_FLUSH` | 刷新解碼器 |
| `RECOVERY_TYPE_RESTART` | 停止並重新配置 |
| `RECOVERY_TYPE_RESET` | 重置解碼器 |

**線程靜止標誌**:
- `FLAG_INPUT_THREAD` - 輸入處理線程
- `FLAG_RENDER_THREAD` - 渲染線程
- `FLAG_CHOREOGRAPHER` - Choreographer 回調

### 3. CsdBufferProcessor

**檔案位置**: `decoder/CsdBufferProcessor.java`

**職責**:
- 處理編解碼器特定數據 (CSD) 緩衝區
- VPS（視頻參數集）處理 - HEVC
- SPS（序列參數集）處理和修補
- PPS（圖片參數集）處理
- H.264 SPS 級別和參考幀修補
- 比特流限制添加

**H.264 SPS 修補**:
- 級別 IDC 調整（根據分辨率和幀率）
- 參考幀數量設置
- VUI 參數和比特流限制
- 約束高配置文件標誌

### 4. PerformanceStatsManager

**檔案位置**: `decoder/PerformanceStatsManager.java`

**職責**:
- 收集幀統計數據（接收、渲染、丟失）
- 解碼器時間追蹤
- 主機處理延遲統計
- 生成性能覆蓋層文本

**統計窗口**:
- 活動窗口：當前 1 秒窗口
- 上一窗口：前一個 1 秒窗口
- 全局統計：累積統計

### 5. DecoderExceptions

**檔案位置**: `decoder/DecoderExceptions.java`

**職責**:
- 自定義異常類用於錯誤報告
- 詳細的渲染器狀態捕獲用於調試

**異常類型**:
- `DecoderHungException` - 解碼器掛起檢測
- `RendererException` - 帶有詳細診斷信息的包裝異常
- `RendererState` - 用於異常報告的狀態快照

## 數據流

### 視頻幀處理流程

```
1. 接收視頻數據
   └── submitDecodeUnit()
       ├── 更新幀統計 (PerformanceStatsManager)
       ├── 處理 IDR 幀 CSD (CsdBufferProcessor)
       │   ├── VPS (HEVC)
       │   ├── SPS (修補 + 存儲)
       │   └── PPS
       ├── 獲取輸入緩衝區
       ├── 提交到 MediaCodec
       └── 返回結果

2. 渲染輸出
   └── MediaCodecDecoderRenderer
       ├── 渲染線程：dequeueOutputBuffer
       ├── Choreographer：幀節奏控制
       └── releaseOutputBuffer（渲染或丟棄）

3. 異常處理
   └── CodecRecoveryManager
       ├── 檢測異常類型
       ├── 選擇恢復策略
       ├── 協調線程靜止
       └── 執行恢復
```

## 線程模型

| 線程 | 職責 |
|------|------|
| 調用線程 | `submitDecodeUnit()` - 提交輸入數據 |
| 渲染線程 | `dequeueOutputBuffer()` - 獲取和渲染輸出 |
| Choreographer 線程 | 幀節奏控制（平衡模式） |

**線程同步**:
- `CodecRecoveryManager` 使用靜止標誌協調所有線程
- 恢復操作需要所有線程達到檢查點

## 配置選項

### 視頻格式
- H.264 (AVC)
- H.265 (HEVC)
- AV1

### 性能選項
- 參考幀失效 (RFI)
- 直接提交
- 自適應播放
- 融合 IDR 幀

### HDR 支持
- HEVC Main10 HDR10
- AV1 Main10 HDR10

## 錯誤處理策略

1. **暫時性錯誤** - 記錄警告，繼續運行
2. **可恢復錯誤** - 嘗試重啟解碼器
3. **不可恢復錯誤** - 嘗試重置，然後重建
4. **持久性錯誤** - 3 秒延遲後報告崩潰

## 向後兼容性

重構保持了 `MediaCodecDecoderRenderer` 的所有公共 API 不變：
- `setup()` / `start()` / `stop()` / `cleanup()`
- `submitDecodeUnit()`
- `getCapabilities()`
- 所有查詢方法（`isHevcSupported()` 等）

## 文件結構

```
binding/video/
├── MediaCodecDecoderRenderer.java  (主類 - 簡化後)
├── MediaCodecHelper.java           (解碼器輔助方法)
├── VideoStats.java                 (統計數據結構)
├── CrashListener.java              (崩潰回調接口)
├── PerfOverlayListener.java        (性能覆蓋層接口)
└── decoder/
    ├── DecoderCapabilityChecker.java
    ├── CodecRecoveryManager.java
    ├── CsdBufferProcessor.java
    ├── PerformanceStatsManager.java
    └── DecoderExceptions.java
```

