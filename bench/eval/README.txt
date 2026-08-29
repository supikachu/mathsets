评测落盘目录（开发用，不进教师产品）。

每份试卷一个子目录，文件名来自 PDF 去扩展名：

  bench/eval/<试卷名>/paper.md     MinerU OCR 原文（拿去跑站外模型）
  bench/eval/<试卷名>/full.json    全自动 structured questions
  bench/eval/<试卷名>/export.json  站外模型返回的 {"questions":[...]}
  bench/eval/<试卷名>/gold.json    人工确认的结构 gold（不要用 export 或库内题代替）
  bench/eval/<试卷名>/chunks.jsonl 切块原文（无运行时切块时 slice_source=eval_reparse）
  bench/eval/<试卷名>/meta.json    document_id / task_id / 真实 ocr_engine / 双 prompt hash
  bench/eval/<试卷名>/report.json  规则分机器可读结果
  bench/eval/<试卷名>/report.md    规则分可读报告
  bench/eval/prompt.md             落盘当时的 docs/rules-prompts.md
  bench/eval/manifest.json         本轮清单
  bench/eval/report_latest.json    最近一次扫全目录的总清单（含按题加权）

站外模型请使用同目录的 paper.md + 上级 prompt.md。
返回的 JSON（{"questions":[...]}）请存为同目录 export.json。
若 dump 时 paper.md 的 sha 变了，旧 export.json 会改名为 export.stale.json。

全自动压测成功后会自动写入 paper.md / full.json；已跑过的任务不必再解析：

  python scripts/bench_full_auto_parse.py --dump-from bench/out/after_latest.json

同一 OCR 上重跑结构化（须上一轮已有 ocr_markdown）：

  python scripts/bench_full_auto_parse.py --label after --reuse bench/out/baseline_latest.json

对齐与规则分（不调用 LLM，不改 slice / 提示词）。缺 export.json 仍评全自动：

  python scripts/bench_eval_quality.py
  python scripts/bench_eval_quality.py --dir bench/eval/某试卷名
  python scripts/bench_eval_quality.py --self-check
  python scripts/bench_eval_quality.py --dir bench/evalset/v0

错因与建议草案（不写回仓库）：

  python scripts/bench_eval_attribute.py --dir bench/evalset/v0
  python scripts/bench_eval_advise.py --dir bench/evalset/v0

规则语义见 docs/全自动解析质量评估_评测规则.md。
保真评测（站外裁判）沟通词见 docs/全自动解析质量评估_裁判沟通词.md。
错误样本库、gold、归因建议与 100/500/1000 题评测集见 docs/全自动解析质量评估_闭环可行性.md。
