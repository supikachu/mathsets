#!/usr/bin/env python3
"""R5 负对照生成器：剥掉 docx 里全部 `w:keepNext`，产出一份**除分页提示外完全相同**的副本。

为什么需要它：`check_keepnext.ps1` 报 0 违例本身说明不了任何事 —— 如果夹具里每道题本来就
独占一页，keepNext 有没有效果都一样。只有「探针 violations=0 且负对照 violations>0」这一组
对比才能把结论归因到 keepNext 上。

用法：
    python scripts/strip_keepnext.py target/t27_keepnext_probe.docx
    # -> target/t27_keepnext_probe.no_keepnext.docx

退出码：0 = 剥掉了至少一处；1 = 一处都没剥到（对照等于原文件，跑它没有意义）。
"""

import re
import sys
import zipfile

# 只认自闭合形态；writer 与静态部件里的 keepNext 都是 <w:keepNext/>
KEEP_NEXT = re.compile(rb"<w:keepNext\s*/>")


def strip(src: str, dst: str) -> int:
    removed = 0
    with zipfile.ZipFile(src) as zin:
        with zipfile.ZipFile(dst, "w", zipfile.ZIP_DEFLATED) as zout:
            for info in zin.infolist():
                data = zin.read(info.filename)
                if info.filename.startswith("word/") and info.filename.endswith(".xml"):
                    data, n = KEEP_NEXT.subn(b"", data)
                    removed += n
                # 保留原名与顺序：OPC 靠 [Content_Types].xml 找部件，不依赖顺序，
                # 但保持顺序能让两份文件除 keepNext 外逐字节可比。
                new = zipfile.ZipInfo(info.filename, date_time=info.date_time)
                new.compress_type = zipfile.ZIP_DEFLATED
                new.external_attr = info.external_attr
                zout.writestr(new, data)
    return removed


def main(argv: list[str]) -> int:
    if len(argv) < 2:
        print(__doc__)
        return 2
    src = argv[1]
    dst = argv[2] if len(argv) > 2 else src[: -len(".docx")] + ".no_keepnext.docx"
    removed = strip(src, dst)
    print(f"src={src}\ndst={dst}\nkeepNext_removed={removed}")
    if removed == 0:
        print("CONTROL IDENTICAL TO PROBE - not a control")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
