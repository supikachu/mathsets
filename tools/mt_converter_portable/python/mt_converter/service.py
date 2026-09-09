"""Lightweight HTTP Microservice for MathType Binary to WMF Conversion.

Supports:
1. FastAPI + Uvicorn server (Preferred).
2. Native Python http.server fallback (zero dependencies).
"""

from __future__ import annotations

import base64
import json
import logging
import sys
from http.server import HTTPServer, BaseHTTPRequestHandler
from typing import Optional, Dict, Any

from .core import MathTypeConverter, ConversionResult

try:
    from pydantic import BaseModel

    class ConvertRequest(BaseModel):
        bin_base64: str

    class ConvertResponse(BaseModel):
        wmf_base64: str
        baseline_offset_pt: float
        width_pt: float = 0.0
        height_pt: float = 0.0
        code: int = 0
        method: Optional[str] = None
        error: Optional[str] = None

    class BatchItem(BaseModel):
        id: str
        bin_base64: str

    class ConvertBatchRequest(BaseModel):
        equations: list[BatchItem]

    class BatchResultItem(BaseModel):
        id: str
        wmf_base64: str = ""
        baseline_offset_pt: float = 0.0
        width_pt: float = 0.0
        height_pt: float = 0.0
        code: int = 0
        method: Optional[str] = None
        error: Optional[str] = None

    class ConvertBatchResponse(BaseModel):
        code: int = 0
        total: int = 0
        results: list[BatchResultItem]
except ImportError:
    ConvertRequest = None  # type: ignore
    ConvertResponse = None  # type: ignore
    BatchItem = None  # type: ignore
    ConvertBatchRequest = None  # type: ignore
    BatchResultItem = None  # type: ignore
    ConvertBatchResponse = None  # type: ignore

logger = logging.getLogger("MathTypeService")

# Global singleton converter instance
_CONVERTER: Optional[MathTypeConverter] = None


def get_converter(core_dir: Optional[str] = None) -> MathTypeConverter:
    global _CONVERTER
    if _CONVERTER is None:
        _CONVERTER = MathTypeConverter(core_dir=core_dir)
    return _CONVERTER


# ---------------------------------------------------------------------------
# FastAPI Microservice Implementation
# ---------------------------------------------------------------------------

def create_fastapi_app(core_dir: Optional[str] = None):
    try:
        from fastapi import FastAPI
    except ImportError:
        logger.warning("FastAPI not available; fallback to standard http.server.")
        return None

    app = FastAPI(
        title="MathType Binary to WMF Service",
        version="1.0.0",
        description="High-performance Windows microservice for converting MathType .bin/MTEF to WMF vector with baseline offset.",
    )

    converter = get_converter(core_dir)

    @app.get("/")
    def index():
        return {
            "name": "MathType WMF Vector Converter Service",
            "status": "ready",
            "core_dir": converter.core_dir,
            "fonts_loaded": len(converter.font_manager.loaded_fonts),
        }

    @app.get("/health")
    def health():
        return {
            "status": "ok",
            "mathtype_core": converter.core_dir,
            "fonts_count": len(converter.font_manager.loaded_fonts),
        }

    @app.post("/convert", response_model=ConvertResponse)
    def convert_formula(payload: ConvertRequest):
        try:
            if not payload.bin_base64:
                return ConvertResponse(
                    wmf_base64="",
                    baseline_offset_pt=0.0,
                    code=1,
                    error="bin_base64 string is empty.",
                )

            raw_bytes = base64.b64decode(payload.bin_base64)
            result: ConversionResult = converter.convert(raw_bytes)
            wmf_b64 = base64.b64encode(result.wmf_bytes).decode("ascii")

            return ConvertResponse(
                wmf_base64=wmf_b64,
                baseline_offset_pt=result.baseline_offset_pt,
                width_pt=result.width_pt,
                height_pt=result.height_pt,
                code=0,
                method=result.method,
            )
        except Exception as e:
            logger.exception("Conversion failed")
            return ConvertResponse(
                wmf_base64="",
                baseline_offset_pt=0.0,
                code=1,
                error=str(e),
            )

    @app.post("/convert_batch", response_model=ConvertBatchResponse)
    def convert_batch_formulas(payload: ConvertBatchRequest):
        try:
            if not payload.equations:
                return ConvertBatchResponse(code=0, total=0, results=[])

            items_to_convert = []
            for eq in payload.equations:
                raw_bytes = base64.b64decode(eq.bin_base64)
                items_to_convert.append((eq.id, raw_bytes))

            raw_results = converter.convert_batch(items_to_convert)
            res_items = []
            for eq_id, res, err in raw_results:
                if res is not None:
                    wmf_b64 = base64.b64encode(res.wmf_bytes).decode("ascii")
                    res_items.append(
                        BatchResultItem(
                            id=eq_id,
                            wmf_base64=wmf_b64,
                            baseline_offset_pt=res.baseline_offset_pt,
                            width_pt=res.width_pt,
                            height_pt=res.height_pt,
                            code=0,
                            method=res.method,
                        )
                    )
                else:
                    res_items.append(
                        BatchResultItem(
                            id=eq_id,
                            code=1,
                            error=err or "Conversion failed",
                        )
                    )
            return ConvertBatchResponse(code=0, total=len(res_items), results=res_items)
        except Exception as e:
            logger.exception("Batch conversion failed")
            return ConvertBatchResponse(code=1, total=0, results=[])

    return app


# ---------------------------------------------------------------------------
# Native http.server Fallback Implementation
# ---------------------------------------------------------------------------

class NativeHttpHandler(BaseHTTPRequestHandler):
    converter: MathTypeConverter

    def _send_json(self, status_code: int, data: Dict[str, Any]):
        body = json.dumps(data, ensure_ascii=False).encode("utf-8")
        self.send_response(status_code)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        if self.path in ("/", "/health"):
            self._send_json(200, {
                "status": "ok",
                "core_dir": self.converter.core_dir,
                "fonts_loaded": len(self.converter.font_manager.loaded_fonts),
            })
        else:
            self._send_json(404, {"error": "Not Found", "code": 404})

    def do_POST(self):
        if self.path not in ("/convert", "/convert_batch"):
            self._send_json(404, {"error": "Endpoint not found. Use POST /convert or /convert_batch", "code": 404})
            return

        content_len = int(self.headers.get("Content-Length", 0))
        post_data = self.rfile.read(content_len)

        try:
            req_json = json.loads(post_data.decode("utf-8"))
            if self.path == "/convert":
                bin_base64 = req_json.get("bin_base64", "")
                if not bin_base64:
                    self._send_json(200, {
                        "wmf_base64": "",
                        "baseline_offset_pt": 0.0,
                        "code": 1,
                        "error": "Missing bin_base64 field in request.",
                    })
                    return

                raw_bytes = base64.b64decode(bin_base64)
                res = self.converter.convert(raw_bytes)
                wmf_b64 = base64.b64encode(res.wmf_bytes).decode("ascii")

                self._send_json(200, {
                    "wmf_base64": wmf_b64,
                    "baseline_offset_pt": res.baseline_offset_pt,
                    "width_pt": res.width_pt,
                    "height_pt": res.height_pt,
                    "code": 0,
                    "method": res.method,
                })
            elif self.path == "/convert_batch":
                equations = req_json.get("equations", [])
                items_to_convert = []
                for eq in equations:
                    eq_id = eq.get("id", "")
                    raw_b64 = eq.get("bin_base64", "")
                    raw_bytes = base64.b64decode(raw_b64)
                    items_to_convert.append((eq_id, raw_bytes))

                raw_results = self.converter.convert_batch(items_to_convert)
                res_items = []
                for eq_id, res, err in raw_results:
                    if res is not None:
                        wmf_b64 = base64.b64encode(res.wmf_bytes).decode("ascii")
                        res_items.append({
                            "id": eq_id,
                            "wmf_base64": wmf_b64,
                            "baseline_offset_pt": res.baseline_offset_pt,
                            "width_pt": res.width_pt,
                            "height_pt": res.height_pt,
                            "code": 0,
                            "method": res.method,
                        })
                    else:
                        res_items.append({
                            "id": eq_id,
                            "wmf_base64": "",
                            "baseline_offset_pt": 0.0,
                            "code": 1,
                            "error": err or "Conversion failed",
                        })
                self._send_json(200, {
                    "code": 0,
                    "total": len(res_items),
                    "results": res_items,
                })
        except Exception as e:
            logger.exception("Native HTTP server conversion error")
            self._send_json(200, {
                "code": 1,
                "error": str(e),
            })

    def log_message(self, format, *args):
        logger.info("%s - - [%s] %s" % (self.client_address[0], self.log_date_time_string(), format % args))


def run_server(host: str = "127.0.0.1", port: int = 8099, core_dir: Optional[str] = None):
    """Run HTTP microservice, preferring FastAPI+Uvicorn and falling back to http.server."""
    converter = get_converter(core_dir)
    app = create_fastapi_app(core_dir)

    if app is not None:
        try:
            import uvicorn
            print(f"[*] Starting FastAPI MathType microservice on http://{host}:{port} ...")
            print(f"[*] Core Dir: {converter.core_dir}")
            print(f"[*] Fonts Loaded: {len(converter.font_manager.loaded_fonts)}")
            uvicorn.run(app, host=host, port=port, log_level="info")
            return
        except Exception as e:
            logger.warning(f"Uvicorn startup failed: {e}. Falling back to standard http.server.")

    # Fallback to standard library http.server
    print(f"[*] Starting standard http.server MathType microservice on http://{host}:{port} ...")
    NativeHttpHandler.converter = converter
    server = HTTPServer((host, port), NativeHttpHandler)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        print("\n[*] Shutting down server...")
    finally:
        server.server_close()
        converter.shutdown()
