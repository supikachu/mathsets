"""Comprehensive Automated Test Suite for MathType Converter.

Validates:
1. Dynamic font loading (FR_PRIVATE)
2. Format auto-detection and MTEF extraction
3. Direct conversion and baseline accuracy across sample equations
4. Fallback 1: Clipboard to file
5. Fallback 2: Clipboard to clipboard with GDI APM assembly
6. FastAPI HTTP microservice API (/health, /convert)
"""

import base64
import os
import sys
import unittest
from fastapi.testclient import TestClient

from mt_converter.core import MathTypeConverter
from mt_converter.mtef_parser import extract_mtef_from_bytes, parse_wmf_dimensions_and_baseline
from mt_converter.service import create_fastapi_app, get_converter

SAMPLES_DIR = r"C:\Users\pikachu\Desktop\mathset\tools\mathtype-ole\samples\out"


class TestMathTypeConverter(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.converter = get_converter()
        print(f"\n[TEST SUITE] Core dir: {cls.converter.core_dir}")
        print(f"[TEST SUITE] Loaded fonts: {len(cls.converter.font_manager.loaded_fonts)}")

    @classmethod
    def tearDownClass(cls):
        cls.converter.shutdown()

    def test_01_fonts_loaded_privately(self):
        """Verify session-level private fonts were loaded via AddFontResourceExW."""
        fonts = self.converter.font_manager.loaded_fonts
        self.assertGreater(len(fonts), 0, "No fonts loaded")
        font_names = [os.path.basename(f).lower() for f in fonts]
        self.assertTrue(any("mtextra" in f or "mt extra" in f for f in font_names))
        print(f"  [PASS] Successfully loaded {len(fonts)} private fonts.")

    def test_02_format_extraction(self):
        """Verify extraction of MTEF from OLE CFB compound files."""
        sample_path = os.path.join(SAMPLES_DIR, "01-frac.bin")
        if not os.path.exists(sample_path):
            self.skipTest("Sample file not found.")

        with open(sample_path, "rb") as f:
            cfb_data = f.read()

        mtef = extract_mtef_from_bytes(cfb_data)
        self.assertEqual(mtef[0], 5, "Expected MTEF v5")

        mtef_raw = extract_mtef_from_bytes(mtef)
        self.assertEqual(mtef, mtef_raw)
        print(f"  [PASS] MTEF extraction verified (length={len(mtef)} bytes).")

    def test_03_direct_conversion_and_baselines(self):
        """Verify conversion across all sample equations and compare with known baselines."""
        cases = [
            ("01-frac.bin", 12.0),
            ("02-sqrt-sup.bin", 4.0),
            ("03-fence.bin", 15.0),
            ("04-inline.bin", 3.0),
        ]

        for fname, expected_baseline in cases:
            fpath = os.path.join(SAMPLES_DIR, fname)
            if not os.path.exists(fpath):
                continue

            with open(fpath, "rb") as f:
                data = f.read()

            res = self.converter.convert(data)
            self.assertEqual(res.code, 0)
            self.assertGreater(len(res.wmf_bytes), 200)
            self.assertAlmostEqual(res.baseline_offset_pt, expected_baseline, places=1)
            print(
                f"  [PASS] {fname}: baseline={res.baseline_offset_pt} pt "
                f"(expected {expected_baseline} pt), size={len(res.wmf_bytes)} bytes"
            )

    def test_04_clipboard_fallback_modes(self):
        """Explicitly test both clipboard fallback paths."""
        sample_path = os.path.join(SAMPLES_DIR, "01-frac.bin")
        if not os.path.exists(sample_path):
            self.skipTest("Sample file not found.")

        with open(sample_path, "rb") as f:
            data = f.read()
        mtef = extract_mtef_from_bytes(data)

        # Fallback 1: Clipboard to File
        res1 = self.converter.execute_on_worker(self.converter._convert_clipboard_to_file, mtef)
        self.assertEqual(res1.method, "clipboard_relay_to_file")
        self.assertAlmostEqual(res1.baseline_offset_pt, 12.0, places=1)
        self.assertGreater(len(res1.wmf_bytes), 200)

        # Fallback 2: Clipboard to Clipboard to GDI assembly
        res2 = self.converter.execute_on_worker(self.converter._convert_clipboard_to_clipboard, mtef)
        self.assertEqual(res2.method, "clipboard_relay_to_clipboard")
        self.assertAlmostEqual(res2.baseline_offset_pt, 12.0, places=1)
        self.assertGreater(len(res2.wmf_bytes), 200)
        print("  [PASS] Both clipboard fallback modes verified successfully.")

    def test_05_http_microservice_api(self):
        """Test FastAPI HTTP microservice /convert and /health endpoints."""
        app = create_fastapi_app(self.converter.core_dir)
        self.assertIsNotNone(app)

        client = TestClient(app)

        # Health
        h = client.get("/health")
        self.assertEqual(h.status_code, 200)
        self.assertEqual(h.json()["status"], "ok")

        # Convert
        sample_path = os.path.join(SAMPLES_DIR, "01-frac.bin")
        with open(sample_path, "rb") as f:
            b64_str = base64.b64encode(f.read()).decode("ascii")

        resp = client.post("/convert", json={"bin_base64": b64_str})
        self.assertEqual(resp.status_code, 200)
        body = resp.json()
        self.assertEqual(body["code"], 0)
        self.assertAlmostEqual(body["baseline_offset_pt"], 12.0, places=1)

        wmf_bytes = base64.b64decode(body["wmf_base64"])
        meta = parse_wmf_dimensions_and_baseline(wmf_bytes)
        self.assertTrue(meta["is_placeable"])
        print(f"  [PASS] FastAPI /convert endpoint returned valid WMF and 12.0 pt baseline.")

    def test_06_http_batch_api(self):
        """Test FastAPI HTTP microservice /convert_batch endpoint with 4 equations."""
        app = create_fastapi_app(self.converter.core_dir)
        self.assertIsNotNone(app)
        client = TestClient(app)

        test_files = [
            ("01-frac.bin", 12.0),
            ("02-sqrt-sup.bin", 4.0),
            ("03-fence.bin", 15.0),
            ("04-inline.bin", 3.0),
        ]
        equations = []
        for idx, (fname, expected_base) in enumerate(test_files):
            fpath = os.path.join(SAMPLES_DIR, fname)
            with open(fpath, "rb") as f:
                b64 = base64.b64encode(f.read()).decode("ascii")
            equations.append({"id": f"eq_{idx}", "bin_base64": b64})

        resp = client.post("/convert_batch", json={"equations": equations})
        self.assertEqual(resp.status_code, 200)
        data = resp.json()
        self.assertEqual(data["code"], 0)
        self.assertEqual(data["total"], 4)

        for idx, (fname, expected_base) in enumerate(test_files):
            item = data["results"][idx]
            self.assertEqual(item["code"], 0)
            self.assertAlmostEqual(item["baseline_offset_pt"], expected_base, places=1)
            wmf_data = base64.b64decode(item["wmf_base64"])
            meta = parse_wmf_dimensions_and_baseline(wmf_data)
            self.assertTrue(meta["is_placeable"])

        print(f"  [PASS] FastAPI /convert_batch successfully converted all {len(equations)} equations in one pass.")


if __name__ == "__main__":
    unittest.main(verbosity=2)
