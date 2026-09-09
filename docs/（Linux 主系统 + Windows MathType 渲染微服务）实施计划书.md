# 方案 A（Linux 主系统 + Windows MathType 渲染微服务）实施计划书

本计划书详细阐述如何在后续将**题库/试卷导出主业务部署于 Linux 云原生环境**，同时通过**独立的轻量 Windows 渲染微服务节点**保障 100% 官方 MathType 公式兼容性（双击可编辑、印刷级基线对齐、高保真 WMF）。

---

## 一、 架构总览与职责划分

本架构采用典型的**“计算密集型微服务分离模式”**，将无状态的公式排版渲染与上游的业务逻辑彻底解耦。

```mermaid
flowchart TD
    subgraph Linux["Linux 主业务环境 (Cloud Native / K8s)"]
        A["题库/组卷导出请求 (如 full.md)"] --> B["解析器: LaTeX -> MathML -> MTEF"]
        B --> C["合成 OLE 编辑源: MTEF -> oleObject.bin"]
        B --> D["公式去重 & Redis 缓存层检索"]
        D -- "未命中缓存的公式 (分片批处理)" --> E["HTTP 客户端 (连接池 / Scatter-Gather)"]
        G["回填 Redis 缓存"] <-- E
        F["DOCX 组装引擎 (注入 w:position / top)"] <-- C
        F <-- D
        F <-- E
        F --> H["生成最终 .docx 试卷文件"]
    end

    subgraph Windows["Windows 渲染计算节点 (轻量 VPC 虚拟机)"]
        E -- "内网专线 HTTP POST /convert_batch" --> I["本地 Nginx 负载均衡 (Port 8090)"]
        I --> J1["Worker 1 (Port 8091)"]
        I --> J2["Worker 2 (Port 8092)"]
        I --> J3["Worker 3 (Port 8093)"]
        I --> J4["Worker N... (Port 8098)"]
        J1 & J2 & J3 & J4 --> K["MT6.dll 纯内存渲染 + 基线解析"]
        K -- "返回 WMF 字节流 + BaselineOffset" --> I
    end
```

### 1. Linux 节点职责（主导 95% 业务逻辑，极速纯内存处理）
- **公式语法转换**：`LaTeX -> MathML -> MTEF`（纯算法执行，耗时 < 0.2ms/个）；
- **OLE 编辑容器合成**：由 `MathTypeOleStorage` 纯算法生成 `word/embeddings/oleObject.bin`，保留双击唤醒 MathType 的核心二进制，**完全不依赖 Windows**；
- **单卷与跨卷去重**：单卷天然去重 28%（如 `full.md` 从 744 个降为 534 个）；
- **全局缓存管理**：Redis 存储 `Hash(MTEF) -> { wmf_bytes, baseline_offset_pt, width_pt, height_pt }`；
- **分发调度（Scatter-Gather）**：将未命中的公式切片并发调用 Windows 渲染池；
- **Word OOXML 组装**：按照此前修复的 `MathTypeDocxWriter` 规范，注入 `<w:position>` 与 `<v:shape top>`。

### 2. Windows 节点职责（纯算力节点，无状态微服务）
- **核心依赖**：绿色便携目录 `mathtype_core/`（包含 `MT6.dll`、`MathType.exe`、私有字体库）；
- **服务载体**：由 PyInstaller 打包的 `mt_converter.exe`；
- **运行模式**：单机启动 8 个无状态进程池（端口 `8091~8098`），前置 Nginx（端口 `8090`）做本地轮询；
- **核心功能**：接收 MTEF 字节流，调用 `MTXFormEqn` 在内存中生成 WMF 并提取基线，回传结果。

---

## 二、 通信协议与接口设计（Batch API）

为避免长文档（如 `full.md` 含 744 个公式）产生 700 多次网络握手，必须在 Windows 服务端引入**批量转换接口**。

### 接口定义：`POST /convert_batch`

#### 请求格式 (JSON)
```json
{
  "equations": [
    {
      "id": "eq_001",
      "mtef_base64": "BQEBAQA..."
    },
    {
      "id": "eq_002",
      "mtef_base64": "BQEBAQB..."
    }
  ]
}
```

#### 响应格式 (JSON)
```json
{
  "code": 0,
  "results": [
    {
      "id": "eq_001",
      "wmf_base64": "183Gmg...",
      "baseline_offset_pt": 12.0,
      "width_pt": 12.0,
      "height_pt": 31.0,
      "status": "ok"
    },
    {
      "id": "eq_002",
      "wmf_base64": "183Gmh...",
      "baseline_offset_pt": 3.0,
      "width_pt": 42.0,
      "height_pt": 16.0,
      "status": "ok"
    }
  ]
}
```

> [!TIP]
> 针对超大规模导出，可启用 HTTP Gzip/Zstandard 压缩，网络传输开销可降低 70% 以上。

---

## 三、 多级缓存与分发调度算法

针对高密度试卷（如 744 个公式），Linux 端调度流程如下：

```
                    [ 原始 744 个公式 ]
                             │
            ┌────────────────┴────────────────┐
            ▼                                 ▼
      [ 210 个卷内重复公式 ]           [ 534 个唯一公式 ]
      (直接引用首次计算结果)                   │
                                              ▼
                                      [ 检索 Redis 缓存 ]
                                              │
                      ┌───────────────────────┴───────────────────────┐
                      ▼                                               ▼
               [ 命中缓存 (如 300 个) ]                    [ 未命中缓存 (如 234 个) ]
               (0.5ms 直接读内存)                             │ (切分为 8 份批处理请求)
                                                              ▼
                                                 [ Windows 节点 8 进程并发处理 ]
                                                 (并发耗时: 234 / 8 * 15ms ≈ 438ms)
                                                              │
                                                              ▼
                                                 [ 异步写回 Redis，并组装 DOCX ]
```

---

## 四、 详细实施步骤

### 阶段 1：Windows 渲染服务扩展与池化（预计 1 天）
1. **服务端接口扩展**：
   - 在 `mt_converter/service.py` 中新增 `POST /convert_batch` 路由；
   - 优化 STA Worker 线程，支持单次任务批量连续转换，避免重复锁竞争。
2. **多进程守护部署**：
   - 编写 Windows 启动脚本（`start_pool.ps1`），通过后台守护进程拉起 8 个端口（8091 ~ 8098）；
   - 编写进程保活机制（单进程异常退出时自动拉起）。
3. **本地 Nginx 负载均衡配置**：
   - 配置 `nginx.conf` 的 `upstream mathtype_pool`，分配轮询权重与健康检查。

### 阶段 2：Linux 端客户端 SDK 与缓存层集成（预计 1~2 天）
1. **HTTP 客户端封装**：
   - 封装高性能异步 HTTP 客户端（基于连接池，长连接复用）；
   - 实现**扇出-聚合（Scatter-Gather）**逻辑：按并发 Worker 数量将长列表公式切片分发。
2. **Redis 缓存键设计**：
   - Cache Key: `mathtype:v1:{sha256(mtef_bytes)}`；
   - 设置合理的 TTL（题库高频公式建议永久或 30 天持久缓存）。
3. **全流程管道联调**：
   - 将 `LaTeX/MathML -> MTEF -> oleObject.bin` 与 `Windows 回传 WMF + baseline` 串联；
   - 调用已修复的 `MathTypeDocxWriter` 完成最终的 DOCX 打包。

### 阶段 3：全链路压测与故障自愈演练（预计 1 天）
1. **基准测试**：使用 `full.md`（744 公式）执行单卷导出，验证全流程端到端耗时（目标压至 **< 1.5 秒**）。
2. **高并发压测**：使用 Jmeter / Locust 模拟 50 并发同时导出整卷试卷（总计并发处理 3.7 万个公式），记录 Windows 节点的 CPU、内存与网络吞吐。
3. **容错自愈测试**：随机 `taskkill` 杀掉 1~2 个 Windows Worker 进程，验证 Nginx 自动摘除节点并在新进程拉起后恢复。

---

## 五、 关键决策与用户确认事项

> [!IMPORTANT]
> **需要确认的部署环境细节：**
> 1. **网络拓扑**：Linux 业务集群与 Windows 渲染机是否处于同一个内网 VPC（建议内网通信，延迟 < 1ms）？
> 2. **Windows 节点规模**：初始上线是否采用 1 台 8 核 16G Windows 实例（若每天导出量在 5 万 ~ 10 万套以内，1 台足矣）？
> 3. **持久化缓存选型**：Linux 端现有的缓存中间件（默认使用 Redis 即可）？
