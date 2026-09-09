"""PyInstaller Build Script for MathType Converter.

Builds a single-file executable `mt_converter.exe` and packages it
alongside the portable `mathtype_core/` runtime directory.
"""

from __future__ import annotations

import os
import shutil
import subprocess
import sys


def main():
    root_dir = os.path.dirname(os.path.abspath(__file__))
    dist_dir = os.path.join(root_dir, "dist")
    build_dir = os.path.join(root_dir, "build")
    entrypoint = os.path.join(root_dir, "main.py")

    print("[*] Starting PyInstaller build for MathType Converter...")

    cmd = [
        sys.executable,
        "-m",
        "PyInstaller",
        "--name=mt_converter",
        "--onefile",
        "--console",
        "--clean",
        # Explicit hidden imports for FastAPI & Uvicorn dynamic loading
        "--hidden-import=uvicorn",
        "--hidden-import=uvicorn.logging",
        "--hidden-import=uvicorn.loops",
        "--hidden-import=uvicorn.loops.auto",
        "--hidden-import=uvicorn.protocols",
        "--hidden-import=uvicorn.protocols.http",
        "--hidden-import=uvicorn.protocols.http.auto",
        "--hidden-import=uvicorn.lifespan",
        "--hidden-import=uvicorn.lifespan.on",
        "--hidden-import=fastapi",
        "--hidden-import=pydantic",
        "--hidden-import=olefile",
        "--collect-all=fastapi",
        "--collect-all=uvicorn",
        entrypoint,
    ]

    print(f"[*] Executing command:\n    {' '.join(cmd)}")
    ret = subprocess.run(cmd, cwd=root_dir)

    if ret.returncode != 0:
        print(f"[!] Build failed with exit code: {ret.returncode}")
        sys.exit(ret.returncode)

    exe_path = os.path.join(dist_dir, "mt_converter.exe")
    if os.path.isfile(exe_path):
        size_mb = os.path.getsize(exe_path) / (1024 * 1024)
        print(f"\n[+] SUCCESS! Generated standalone executable:")
        print(f"    Path: {exe_path} ({size_mb:.2f} MB)")

        # Prepare portable distribution folder
        portable_pkg_dir = os.path.join(dist_dir, "mt_converter_portable")
        os.makedirs(portable_pkg_dir, exist_ok=True)
        shutil.copy2(exe_path, os.path.join(portable_pkg_dir, "mt_converter.exe"))

        src_core = os.path.join(root_dir, "mathtype_core")
        dst_core = os.path.join(portable_pkg_dir, "mathtype_core")
        if os.path.isdir(src_core):
            if os.path.exists(dst_core):
                shutil.rmtree(dst_core)
            shutil.copytree(src_core, dst_core)
            print(f"    Copied portable 'mathtype_core/' to: {portable_pkg_dir}")

        print("\n[*] Portable release ready at:")
        print(f"    {portable_pkg_dir}\\mt_converter.exe")
        print("\n[*] Usage:")
        print("    1. CLI Mode:")
        print("       mt_converter.exe --input formula.bin --output formula.wmf --print-baseline")
        print("    2. HTTP Microservice:")
        print("       mt_converter.exe --serve --port 8099")
    else:
        print("[!] Target executable was not found.")
        sys.exit(1)


if __name__ == "__main__":
    main()
