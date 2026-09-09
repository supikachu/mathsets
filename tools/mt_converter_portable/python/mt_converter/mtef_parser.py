"""MathType MTEF / OLE Binary parser and WMF geometry extractor.

Provides robust auto-detection for:
1. Compound File Binary (CFB / OLE2 .bin) containing 'Equation Native' stream.
2. OLE 'Equation Native' stream with 28-byte header.
3. Raw MTEF v1-v5 byte streams.
4. Parsing and synthesis of Aldus Placeable Metafile (APM) headers and baseline comments.
"""

from __future__ import annotations

import io
import struct
from typing import Optional, Tuple, Dict, Any

try:
    import olefile
except ImportError:
    olefile = None

CFB_SIGNATURE = b"\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1"
APM_KEY = 0x9AC6CDD7
META_ESCAPE = 0x0626
MFCOMMENT = 15


def extract_mtef_from_bytes(data: bytes) -> bytes:
    """Extract raw MTEF bytes from various binary packaging formats.

    Supports:
    - OLE CFB document (.bin) with 'Equation Native'
    - Equation Native stream (28-byte header + MTEF)
    - Raw MTEF v1-v5 payload
    """
    if not data or len(data) < 4:
        raise ValueError("Input binary data is empty or too short.")

    # 1. Check for OLE Compound File Binary (CFB)
    if data.startswith(CFB_SIGNATURE):
        if olefile is None:
            raise RuntimeError(
                "olefile library is required to unpack Compound File Binary (.bin). "
                "Please run: pip install olefile"
            )
        try:
            ole = olefile.OleFileIO(io.BytesIO(data))
        except Exception as e:
            raise ValueError(f"Failed to parse OLE compound document: {e}") from e

        stream_name = None
        # Common stream names for MathType
        candidates = [
            "Equation Native",
            "\x03Equation Native",
            "equation native",
            "\x01Equation Native",
        ]
        for c in candidates:
            if ole.exists(c):
                stream_name = c
                break

        if not stream_name:
            for s in ole.listdir():
                if "Equation Native" in s[-1] or "equation native" in s[-1].lower():
                    stream_name = s
                    break

        if not stream_name:
            raise ValueError("No 'Equation Native' stream found in OLE Compound File.")

        eq_stream = ole.openstream(stream_name).read()
        return extract_mtef_from_bytes(eq_stream)

    # 2. Check for OLE Equation Native 28-byte header
    if len(data) >= 28:
        hdr_len = struct.unpack("<H", data[0:2])[0]
        if hdr_len == 28 and len(data) > 28:
            mtef_ver = data[28]
            if mtef_ver in (1, 2, 3, 4, 5):
                # Optionally check object length from offset 8
                obj_len = struct.unpack("<I", data[8:12])[0]
                if 0 < obj_len <= len(data) - 28:
                    return data[28 : 28 + obj_len]
                return data[28:]

    # 3. Check for Raw MTEF byte stream (version 1 to 5)
    if data[0] in (1, 2, 3, 4, 5):
        return data

    # 4. Fallback search for MTEF v5 signature (0x05, 0x01, 0x01, ...)
    idx = data.find(b"\x05\x01\x01")
    if idx != -1:
        return data[idx:]

    raise ValueError(
        "Unrecognized binary format: Expected OLE CFB (.bin), Equation Native stream, or raw MTEF."
    )


def parse_wmf_dimensions_and_baseline(wmf_bytes: bytes) -> Dict[str, Any]:
    """Parse WMF bytes to extract placeable header bounding box and baseline comment.

    Returns dict with:
    - 'width_pt': float (width in typographical points)
    - 'height_pt': float (height in typographical points)
    - 'baseline_offset_pt': Optional[float] (baseline distance from bottom in pt)
    - 'is_placeable': bool
    - 'inch': int (DPI / units per inch)
    """
    if len(wmf_bytes) < 18:
        return {
            "width_pt": 0.0,
            "height_pt": 0.0,
            "baseline_offset_pt": None,
            "is_placeable": False,
            "inch": 2304,
        }

    key = struct.unpack("<I", wmf_bytes[:4])[0]
    is_placeable = key == APM_KEY
    offset = 22 if is_placeable else 0

    width_pt = 0.0
    height_pt = 0.0
    inch = 2304

    if is_placeable and len(wmf_bytes) >= 22:
        _, _, left, top, right, bottom, inch, _, _ = struct.unpack(
            "<IHhhhhHIH", wmf_bytes[:22]
        )
        if inch > 0:
            width_pt = round((right - left) / inch * 72.0, 4)
            height_pt = round((bottom - top) / inch * 72.0, 4)

    baseline_offset_pt = None
    idx = offset + 18
    wmf_len = len(wmf_bytes)

    while idx <= wmf_len - 6:
        rec_size, rec_func = struct.unpack("<IH", wmf_bytes[idx : idx + 6])
        if rec_size == 0:
            break

        rec_byte_len = rec_size * 2
        if idx + rec_byte_len > wmf_len:
            break

        rec_bytes = wmf_bytes[idx : idx + rec_byte_len]

        if rec_func == META_ESCAPE and len(rec_bytes) >= 10:
            esc_func, esc_count = struct.unpack("<HH", rec_bytes[6:10])
            if esc_func == MFCOMMENT and esc_count >= 12 and len(rec_bytes) >= 10 + esc_count:
                comment = rec_bytes[10 : 10 + esc_count]
                if comment.startswith(b"MathType\x00\x00"):
                    # MathType embeds baseline offset in 16ths of a point (signed 16-bit)
                    raw_val = struct.unpack("<h", comment[10:12])[0]
                    baseline_offset_pt = round(raw_val / 16.0, 4)

        idx += rec_byte_len
        if rec_func == 0:  # META_EOF
            break

    return {
        "width_pt": width_pt,
        "height_pt": height_pt,
        "baseline_offset_pt": baseline_offset_pt,
        "is_placeable": is_placeable,
        "inch": inch,
    }


def parse_wmf_baseline(wmf_bytes: bytes) -> Optional[float]:
    """Extract baseline offset from WMF comment records."""
    info = parse_wmf_dimensions_and_baseline(wmf_bytes)
    return info.get("baseline_offset_pt")


def create_placeable_wmf(
    raw_wmf_records: bytes,
    width_units: int,
    height_units: int,
    dpi: int = 2304,
) -> bytes:
    """Wrap raw WMF records in a standard 22-byte Aldus Placeable Metafile header.

    Aldus APM Header (22 bytes):
    - DWORD  key (0x9AC6CDD7)
    - WORD   hmf (0)
    - SHORT  bbox.left (0)
    - SHORT  bbox.top (0)
    - SHORT  bbox.right (width_units)
    - SHORT  bbox.bottom (height_units)
    - WORD   inch (dpi, default 2304 = 32 units/pt)
    - DWORD  reserved (0)
    - WORD   checksum (XOR of previous 10 WORDs)
    """
    left = 0
    top = 0
    right = width_units
    bottom = height_units

    header_wo_checksum = struct.pack(
        "<IHhhhhHI",
        APM_KEY,
        0,
        left,
        top,
        right,
        bottom,
        dpi,
        0,
    )

    # Calculate 16-bit XOR checksum of the first 10 WORDs (20 bytes)
    words = struct.unpack("<10H", header_wo_checksum)
    checksum = 0
    for w in words:
        checksum ^= w

    header = header_wo_checksum + struct.pack("<H", checksum)
    return header + raw_wmf_records
