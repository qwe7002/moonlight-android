# submitDecodeUnit 函數重構總結

## 重構目標
將原本超過 345 行的 `submitDecodeUnit` 函數重構為更小、更易於維護的模塊化函數。

## 重構內容

### 1. 提取的新方法

#### 1.1 統計相關方法
- **updateFrameStats(frameNumber, frameType)**: 
  - 處理幀統計和幀丟失檢測
  - 重置 IDR 幀的 CSD 數據
  
- **updatePerformanceStats()**: 
  - 每秒更新性能統計窗口
  - 觸發性能數據的計算和顯示

- **buildPerformanceStatsString()**: 
  - 構建性能統計字符串
  - 格式化幀率、解碼時間、網絡延遲等信息

- **updateHostProcessingLatencyStats(frameHostProcessingLatency)**: 
  - 更新主機處理延遲統計
  - 計算最小、最大和平均延遲

#### 1.2 IDR 幀 CSD 處理方法
- **handleIdrFrameCsd(decodeUnitType, decodeUnitData, decodeUnitLength)**: 
  - 統一處理 IDR 幀的 CSD (Codec Specific Data) 緩衝區
  - 根據類型分派到具體的處理方法

- **processH264Sps(decodeUnitData, decodeUnitLength)**: 
  - 處理 H.264 SPS (Sequence Parameter Set)
  - 調用 SPS 參數修補方法

- **processVps(decodeUnitData, decodeUnitLength)**: 
  - 處理 VPS (Video Parameter Set)
  - 批量提交 HEVC/H.265 的 CSD 數據

- **processHevcSps(decodeUnitData, decodeUnitLength)**: 
  - 處理 HEVC/H.265 的 SPS

- **processPps(decodeUnitData, decodeUnitLength)**: 
  - 處理 PPS (Picture Parameter Set)

- **submitCsdBuffers()**: 
  - 提交所有 CSD 緩衝區到解碼器
  - 處理 baseline SPS hack

#### 1.3 SPS 修補方法
- **patchSpsParameters(sps)**: 
  - 修補 SPS 的基本參數
  - 設置 level_idc 和參考幀數量

- **patchSpsVuiParameters(sps)**: 
  - 修補 VUI (Video Usability Information) 參數
  - 處理色彩描述和位流限制

- **addOrPatchBitstreamRestrictions(sps)**: 
  - 添加或修補位流限制
  - 設置運動向量和幀緩衝參數

- **handleBaselineSpsHack(sps)**: 
  - 處理 baseline profile hack
  - 保存 SPS 以便後續重放

#### 1.4 幀數據提交方法
- **submitFrameData(decodeUnitData, decodeUnitLength, frameType, enqueueTimeMs, csdSubmittedForThisFrame)**: 
  - 提交實際的幀數據到解碼器
  - 處理 IDR 幀的融合 CSD 數據

- **prepareCodecFlags(frameType, csdSubmittedForThisFrame)**: 
  - 準備編解碼器標誌
  - 處理 IDR 幀的同步標誌和融合 CSD

- **calculateTimestampUs(enqueueTimeMs)**: 
  - 計算並維護單調遞增的時間戳
  - 避免重複時間戳問題

- **validateAndCopyDecodeUnit(decodeUnitData, decodeUnitLength)**: 
  - 驗證解碼單元大小
  - 將數據複製到輸入緩衝區

### 2. 重構後的 submitDecodeUnit 函數結構

```java
public int submitDecodeUnit(...) {
    // 1. 快速檢查是否正在停止
    if (stopping) return MoonBridge.DR_OK;
    
    // 2. 更新幀統計
    updateFrameStats(frameNumber, frameType);
    
    // 3. 更新性能統計
    updatePerformanceStats();
    
    // 4. 處理 IDR 幀的 CSD
    if (frameType == MoonBridge.FRAME_TYPE_IDR) {
        int result = handleIdrFrameCsd(...);
        // 處理結果...
    }
    
    // 5. 更新主機處理延遲統計
    updateHostProcessingLatencyStats(frameHostProcessingLatency);
    
    // 6. 更新其他統計
    activeWindowVideoStats.totalFramesReceived++;
    activeWindowVideoStats.totalFrames++;
    
    // 7. 提交幀數據
    return submitFrameData(...);
}
```

## 重構優勢

### 1. 可讀性提升
- 主函數現在只有約 30 行，邏輯清晰
- 每個子方法都有明確的單一職責
- 方法名稱清楚表達其功能

### 2. 可維護性提升
- 更容易定位和修復特定功能的問題
- 修改某個功能時不會影響其他部分
- 減少了代碼重複

### 3. 可測試性提升
- 每個子方法都可以獨立測試
- 更容易編寫單元測試
- 更容易模擬和驗證行為

### 4. 複雜度降低
- 將 345+ 行的函數分解為多個 10-50 行的小函數
- 減少了嵌套層級
- 降低了認知負擔

## 保持的功能
- 所有原有功能保持不變
- 錯誤處理邏輯保持不變
- 性能特性保持不變
- API 接口保持不變

## 注意事項
- 重構過程中保持了所有的邊界條件檢查
- 保持了原有的錯誤恢復機制
- 沒有改變任何業務邏輯
- 只改進了代碼組織結構

## 後續建議
1. 為新提取的方法添加 JavaDoc 注釋
2. 考慮為關鍵方法添加單元測試
3. 進一步審查是否還有可以優化的地方
4. 監控性能確保重構沒有引入性能問題

