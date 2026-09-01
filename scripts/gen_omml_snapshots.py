#!/usr/bin/env python3
"""MML2OMML 黄金快照生成器（实施计划 T2.3 / 修订 R2）。

微软官方 `MML2OMML.XSL` 是 OMML 的事实规范，但它随 Office 分发、许可不宜入库，
所以仓库只保存**由它生成的事实性输出** `tests/snapshots/*.omml`（固件），XSL 本体走
`assets/xsl/`（已 gitignore）。Rust 侧 `src/export/math/omml.rs` 的实现必须在
XML 规范化后与这些固件逐节点一致（CI 断言，见 T2.4）。

输入：`tests/snapshots/cases/*.mathml` —— Presentation MathML 用例，
      首行 `<!-- latex: ... -->` 记录它对应的 LaTeX 来源。
输出：`tests/snapshots/<用例名>.omml`。

XSL 查找顺序：`--xsl` 参数 → 环境变量 `MML2OMML_XSL` → `assets/xsl/MML2OMML.XSL`
→ 本机 Office 安装目录。

    python scripts/gen_omml_snapshots.py                # 全部用例
    python scripts/gen_omml_snapshots.py --case frac    # 单个用例
    python scripts/gen_omml_snapshots.py --check        # 只校验固件是否与 XSL 一致
"""

from __future__ import annotations

import argparse
import os
import sys
from pathlib import Path

from lxml import etree

REPO = Path(__file__).resolve().parent.parent
CASE_DIR = REPO / "tests" / "snapshots" / "cases"
SNAP_DIR = REPO / "tests" / "snapshots"
XSL_IN_REPO = REPO / "assets" / "xsl" / "MML2OMML.XSL"

# 本机 Office 常见分发位置（只读，不入库）
OFFICE_CANDIDATES = [
    Path(r"C:/Program Files/Microsoft Office/root/Office16/MML2OMML.XSL"),
    Path(r"C:/Program Files (x86)/Microsoft Office/root/Office16/MML2OMML.XSL"),
    Path(r"C:/Program Files/Microsoft Office/Office/MML2OMML.XSL"),
]

MATHML_NS = "http://www.w3.org/1998/Math/MathML"


def find_xsl(cli_path: str | None) -> Path:
    candidates = []
    if cli_path:
        candidates.append(Path(cli_path))
    env = os.environ.get("MML2OMML_XSL")
    if env:
        candidates.append(Path(env))
    candidates.append(XSL_IN_REPO)
    candidates.extend(OFFICE_CANDIDATES)
    for c in candidates:
        if c.is_file():
            return c
    sys.exit(
        "找不到 MML2OMML.XSL。请从本机 Office 目录拷贝到 assets/xsl/MML2OMML.XSL，"
        "或用 --xsl / 环境变量 MML2OMML_XSL 指定路径（详见 assets/xsl/README.md）。"
        f"\n已尝试：{'、'.join(str(c) for c in candidates)}"
    )


def load_stylesheet(xsl: Path) -> etree.XSLT:
    parser = etree.XMLParser(resolve_entities=False, huge_tree=True)
    tree = etree.parse(str(xsl), parser)
    return etree.XSLT(tree)


def read_case(path: Path) -> bytes:
    """去掉 LaTeX 来源注释后交给 XSLT（注释对转换无意义，且会干扰固件稳定性）。"""
    parser = etree.XMLParser(remove_comments=True, remove_blank_text=True)
    tree = etree.parse(str(path), parser)
    if tree.getroot().tag != f"{{{MATHML_NS}}}math":
        sys.exit(f"{path.name} 的根节点不是 <math>，无法喂给 MML2OMML.XSL")
    return etree.tostring(tree)


def render(transform: etree.XSLT, mathml: bytes) -> str:
    src = etree.fromstring(mathml)
    out = transform(src)
    if getattr(out, "error_log", None) and out.error_log.filter_from_errors():
        sys.exit(f"XSLT 转换失败：{out.error_log}")
    text = etree.tostring(out, encoding="unicode", pretty_print=True)
    return text.replace("\r\n", "\n").rstrip() + "\n"


def main() -> int:
    ap = argparse.ArgumentParser(description="生成 / 校验 OMML 黄金快照固件")
    ap.add_argument("--xsl", help="MML2OMML.XSL 路径")
    ap.add_argument("--case", action="append", help="只处理指定用例名（可重复）")
    ap.add_argument("--check", action="store_true", help="只比对，不写文件")
    args = ap.parse_args()

    xsl = find_xsl(args.xsl)
    cases = sorted(CASE_DIR.glob("*.mathml"))
    if args.case:
        wanted = set(args.case)
        cases = [c for c in cases if c.stem in wanted]
        missing = wanted - {c.stem for c in cases}
        if missing:
            sys.exit(f"用例不存在：{'、'.join(sorted(missing))}（用例目录 {CASE_DIR}）")
    if not cases:
        sys.exit(f"用例目录为空：{CASE_DIR}")

    transform = load_stylesheet(xsl)
    SNAP_DIR.mkdir(parents=True, exist_ok=True)
    stale, drifted = [], []
    for case in cases:
        expected = render(transform, read_case(case))
        snap = SNAP_DIR / f"{case.stem}.omml"
        if args.check:
            if not snap.exists():
                stale.append(snap.name)
            elif snap.read_text(encoding="utf-8").replace("\r\n", "\n") != expected:
                drifted.append(snap.name)
            continue
        snap.write_text(expected, encoding="utf-8", newline="\n")
        print(f"  {snap.name:<28} {len(expected):>6} 字符")

    print(f"{'校验' if args.check else '生成'}：{len(cases)} 例 / XSL {xsl}")
    if stale or drifted:
        for name in stale:
            print(f"  缺固件 {name}")
        for name in drifted:
            print(f"  与 XSL 输出不一致 {name}")
        print("→ 去掉 --check 重新生成")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
