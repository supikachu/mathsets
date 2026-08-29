冻结评测集 V0（可入库；脱敏后的 paper.md / full.json / gold.json 放 papers/ 下）。

不要把 inbox 临时 dump 或 PDF 放进来。评测默认：

  python scripts/bench_eval_quality.py --dir bench/evalset/v0
  python scripts/bench_eval_attribute.py --dir bench/evalset/v0
  python scripts/bench_eval_advise.py --dir bench/evalset/v0

冻结步骤（人工）：

1. 修完 P0 后对第一期试卷 `--dump-from`，确认 markdown_sha256 稳定。
2. 把脱敏后的 paper.md / full.json（有则 gold.json、export.json）拷到 papers/<paper_id>/。
3. 更新 manifest.json 的 papers[]、question_count、prompt hash。
4. 失败桶每桶至少人工看 3 题；对齐失败整卷看 1 次。
5. gold 禁止用 export.json 或库内已保存题目。

advice/ 只存放 L2 草案，不是代码补丁。
