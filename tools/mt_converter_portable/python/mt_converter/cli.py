"""Command Line Interface for MathType Binary to WMF Converter."""

from __future__ import annotations

import argparse
import glob
import json
import os
import sys
from typing import Optional

from .core import MathTypeConverter
from .service import run_server


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="mt_converter",
        description="MathType Binary (.bin/MTEF) to WMF Vector Converter with Baseline Extraction (Windows Portable)",
    )
    # Mode selection
    parser.add_argument(
        "--serve",
        action="store_true",
        help="Run as HTTP microservice (Default if no input file is specified).",
    )
    parser.add_argument(
        "--host",
        default="127.0.0.1",
        help="HTTP microservice bind address (default: 127.0.0.1).",
    )
    parser.add_argument(
        "--port",
        type=int,
        default=8099,
        help="HTTP microservice bind port (default: 8099).",
    )

    # CLI conversion options
    parser.add_argument(
        "-i", "--input",
        dest="input_file",
        help="Path to input MathType binary file (.bin, .ole, or raw MTEF).",
    )
    parser.add_argument(
        "-o", "--output",
        dest="output_file",
        help="Path to output WMF file (default: input file name with .wmf extension).",
    )
    parser.add_argument(
        "--print-baseline",
        action="store_true",
        help="Print the calculated baseline offset (in pt) to standard output.",
    )
    parser.add_argument(
        "--json",
        action="store_true",
        help="Output conversion results in JSON format.",
    )
    parser.add_argument(
        "--batch-dir",
        help="Batch convert all .bin/.ole files in a directory.",
    )
    parser.add_argument(
        "--output-dir",
        help="Output directory for batch conversion.",
    )
    parser.add_argument(
        "--core-dir",
        help="Path to portable mathtype_core directory containing MT6.dll and Fonts/.",
    )

    return parser


def run_cli(args: Optional[list[str]] = None) -> int:
    parser = build_parser()
    opts = parser.parse_args(args)

    # If --serve is explicitly requested, or no input arguments given, run server
    if opts.serve or (not opts.input_file and not opts.batch_dir):
        run_server(host=opts.host, port=opts.port, core_dir=opts.core_dir)
        return 0

    try:
        conv = MathTypeConverter(core_dir=opts.core_dir)
    except Exception as e:
        sys.stderr.write(f"[ERROR] Failed to initialize MathTypeConverter: {e}\n")
        return 1

    # Single file conversion
    if opts.input_file:
        if not os.path.isfile(opts.input_file):
            sys.stderr.write(f"[ERROR] Input file not found: {opts.input_file}\n")
            return 1

        out_path = opts.output_file or os.path.splitext(opts.input_file)[0] + ".wmf"
        os.makedirs(os.path.dirname(os.path.abspath(out_path)), exist_ok=True)

        try:
            with open(opts.input_file, "rb") as f:
                in_bytes = f.read()

            res = conv.convert(in_bytes)

            with open(out_path, "wb") as f:
                f.write(res.wmf_bytes)

            if opts.json:
                print(json.dumps({
                    "input": opts.input_file,
                    "output": out_path,
                    "baseline_offset_pt": res.baseline_offset_pt,
                    "width_pt": res.width_pt,
                    "height_pt": res.height_pt,
                    "method": res.method,
                    "code": 0,
                }, ensure_ascii=False, indent=2))
            else:
                print(f"[SUCCESS] Converted '{opts.input_file}' -> '{out_path}' ({len(res.wmf_bytes)} bytes)")
                if opts.print_baseline:
                    print(f"baseline_offset_pt: {res.baseline_offset_pt}")
                else:
                    print(f"  - Dimensions: width={res.width_pt} pt, height={res.height_pt} pt")
                    print(f"  - Baseline Offset: {res.baseline_offset_pt} pt")
                    print(f"  - Method: {res.method}")
            return 0
        except Exception as e:
            sys.stderr.write(f"[ERROR] Conversion failed: {e}\n")
            return 1

    # Batch directory conversion
    if opts.batch_dir:
        if not os.path.isdir(opts.batch_dir):
            sys.stderr.write(f"[ERROR] Batch directory not found: {opts.batch_dir}\n")
            return 1

        out_dir = opts.output_dir or opts.batch_dir
        os.makedirs(out_dir, exist_ok=True)

        pattern_list = ["*.bin", "*.ole", "*.mtef"]
        matched_files = []
        for p in pattern_list:
            matched_files.extend(glob.glob(os.path.join(opts.batch_dir, p)))

        if not matched_files:
            print(f"[*] No matching files found in {opts.batch_dir}")
            return 0

        print(f"[*] Batch converting {len(matched_files)} files to {out_dir} ...")
        success_count = 0
        for fpath in matched_files:
            base_name = os.path.splitext(os.path.basename(fpath))[0]
            target_wmf = os.path.join(out_dir, base_name + ".wmf")
            try:
                with open(fpath, "rb") as f:
                    data = f.read()
                res = conv.convert(data)
                with open(target_wmf, "wb") as f:
                    f.write(res.wmf_bytes)
                print(f"  [OK] {base_name}.wmf | baseline: {res.baseline_offset_pt} pt")
                success_count += 1
            except Exception as e:
                print(f"  [FAIL] {base_name}: {e}")

        print(f"[*] Completed: {success_count}/{len(matched_files)} converted successfully.")
        return 0 if success_count == len(matched_files) else 1

    return 0
