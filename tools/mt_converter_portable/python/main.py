"""MathType Binary to WMF Converter Application Entrypoint."""

import sys
from mt_converter.cli import run_cli

if __name__ == "__main__":
    sys.exit(run_cli())
