"""MathType Core Conversion Engine.

Implements:
1. Dynamic session-level font loading via Windows GDI AddFontResourceExW (FR_PRIVATE).
2. Direct MT6.dll C-API transformation binding.
3. Windows OLE Clipboard fallback transformation path.
4. Baseline offset calculation and verification.
5. Thread-safe connection lifecycle and zombie process recovery.
"""

from __future__ import annotations

import atexit
import concurrent.futures
import ctypes
from ctypes import wintypes
import logging
import os
import queue
import subprocess
import sys
import tempfile
import threading
import time
from dataclasses import dataclass
from typing import Optional, Tuple, List

from .mtef_parser import (
    extract_mtef_from_bytes,
    parse_wmf_dimensions_and_baseline,
    create_placeable_wmf,
)

logger = logging.getLogger("MathTypeConverter")

# ---------------------------------------------------------------------------
# Windows Win32 API Definitions
# ---------------------------------------------------------------------------

FR_PRIVATE = 0x10
GHND = 0x0042  # GMEM_MOVEABLE | GMEM_ZEROINIT
CF_METAFILEPICT = 3

kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
kernel32.SetDllDirectoryW.argtypes = [wintypes.LPCWSTR]
kernel32.SetDllDirectoryW.restype = wintypes.BOOL

kernel32.GlobalAlloc.argtypes = [wintypes.UINT, ctypes.c_size_t]
kernel32.GlobalAlloc.restype = ctypes.c_void_p

kernel32.GlobalLock.argtypes = [ctypes.c_void_p]
kernel32.GlobalLock.restype = ctypes.c_void_p

kernel32.GlobalUnlock.argtypes = [ctypes.c_void_p]
kernel32.GlobalUnlock.restype = wintypes.BOOL

kernel32.GlobalFree.argtypes = [ctypes.c_void_p]
kernel32.GlobalFree.restype = ctypes.c_void_p

user32 = ctypes.WinDLL("user32", use_last_error=True)
user32.OpenClipboard.argtypes = [wintypes.HWND]
user32.OpenClipboard.restype = wintypes.BOOL

user32.CloseClipboard.argtypes = []
user32.CloseClipboard.restype = wintypes.BOOL

user32.EmptyClipboard.argtypes = []
user32.EmptyClipboard.restype = wintypes.BOOL

user32.GetClipboardData.argtypes = [wintypes.UINT]
user32.GetClipboardData.restype = ctypes.c_void_p

user32.SetClipboardData.argtypes = [wintypes.UINT, ctypes.c_void_p]
user32.SetClipboardData.restype = ctypes.c_void_p

user32.RegisterClipboardFormatW.argtypes = [wintypes.LPCWSTR]
user32.RegisterClipboardFormatW.restype = wintypes.UINT

gdi32 = ctypes.WinDLL("gdi32", use_last_error=True)
gdi32.AddFontResourceExW.argtypes = [wintypes.LPCWSTR, wintypes.DWORD, ctypes.c_void_p]
gdi32.AddFontResourceExW.restype = ctypes.c_int

gdi32.RemoveFontResourceExW.argtypes = [wintypes.LPCWSTR, wintypes.DWORD, ctypes.c_void_p]
gdi32.RemoveFontResourceExW.restype = wintypes.BOOL

gdi32.GetMetaFileBitsEx.argtypes = [ctypes.c_void_p, wintypes.UINT, ctypes.c_void_p]
gdi32.GetMetaFileBitsEx.restype = wintypes.UINT

ole32 = ctypes.WinDLL("ole32", use_last_error=True)
ole32.CoInitializeEx.argtypes = [ctypes.c_void_p, wintypes.DWORD]
ole32.CoInitializeEx.restype = ctypes.c_long
ole32.CoUninitialize.argtypes = []
ole32.CoUninitialize.restype = None
COINIT_APARTMENTTHREADED = 0x2


class METAFILEPICT(ctypes.Structure):
    _fields_ = [
        ("mm", wintypes.LONG),
        ("xExt", wintypes.LONG),
        ("yExt", wintypes.LONG),
        ("hMF", ctypes.c_void_p),
    ]


# ---------------------------------------------------------------------------
# MathType SDK Constants & Error Codes
# ---------------------------------------------------------------------------

MT_OK = 0
MT_NOT_FOUND = -1
MT_CANT_RUN = -2
MT_BAD_VERSION = -3
MT_IN_USE = -4
MT_NOT_RUNNING = -5
MT_RUN_TIMEOUT = -6
MT_NOT_EQUATION = -7
MT_FILE_NOT_FOUND = -8
MT_MEMORY = -9
MT_TRANSLATOR_ERROR = -14
MT_PREFERENCE_ERROR = -15
MT_BAD_PATH = -16
MT_ERROR = -9999

MT_INIT_LAUNCH_AS_NEEDED = 0
MT_INIT_LAUNCH_NOW = 1

MTXFM_PREVIOUS = -1
MTXFM_CLIPBOARD = -2
MTXFM_LOCAL = -3
MTXFM_FILE = -4

MTXFM_MTEF = 4
MTXFM_HMTEF = 5
MTXFM_PICT = 6
MTXFM_TEXT = 7
MTXFM_HTEXT = 8
MTXFM_GIF = 9

MTXFM_PREF_EXISTING = 1
MTXFM_PREF_MT_DEFAULT = 2
MTXFM_PREF_USER = 3

MTDIM_WIDTH = 1
MTDIM_HEIGHT = 2
MTDIM_BASELINE = 3

STATUS_DESCRIPTIONS = {
    MT_OK: "mtOK (Success)",
    MT_NOT_FOUND: "mtNOT_FOUND (MathType server/session not found)",
    MT_CANT_RUN: "mtCANT_RUN (Could not start MathType.exe)",
    MT_BAD_VERSION: "mtBAD_VERSION (Version mismatch)",
    MT_IN_USE: "mtIN_USE (MathType server is busy)",
    MT_NOT_RUNNING: "mtNOT_RUNNING (MathType is not running)",
    MT_RUN_TIMEOUT: "mtRUN_TIMEOUT (Timeout waiting for MathType server)",
    MT_NOT_EQUATION: "mtNOT_EQUATION (Corrupted or invalid MTEF data)",
    MT_FILE_NOT_FOUND: "mtFILE_NOT_FOUND",
    MT_MEMORY: "mtMEMORY (Out of memory)",
    MT_TRANSLATOR_ERROR: "mtTRANSLATOR_ERROR",
    MT_PREFERENCE_ERROR: "mtPREFERENCE_ERROR",
    MT_BAD_PATH: "mtBAD_PATH (Destination file path invalid)",
    MT_ERROR: "mtERROR (General internal MathType error)",
}


def describe_status(code: int) -> str:
    return STATUS_DESCRIPTIONS.get(code, f"Unknown MathType status code ({code})")


# ---------------------------------------------------------------------------
# Font Manager (Dynamic FR_PRIVATE Loading)
# ---------------------------------------------------------------------------

class FontManager:
    """Dynamically loads and unloads private session-level fonts without admin rights."""

    def __init__(self, fonts_dir: str):
        self.fonts_dir = os.path.abspath(fonts_dir)
        self.loaded_fonts: List[str] = []
        self._lock = threading.Lock()

    def load_all(self) -> int:
        with self._lock:
            if self.loaded_fonts:
                return len(self.loaded_fonts)

            if not os.path.isdir(self.fonts_dir):
                logger.warning(f"Fonts directory does not exist: {self.fonts_dir}")
                return 0

            count = 0
            for root, _, files in os.walk(self.fonts_dir):
                for f in files:
                    ext = os.path.splitext(f)[1].lower()
                    if ext in (".ttf", ".otf", ".fon"):
                        font_path = os.path.join(root, f)
                        res = gdi32.AddFontResourceExW(font_path, FR_PRIVATE, None)
                        if res > 0:
                            self.loaded_fonts.append(font_path)
                            count += 1
                        else:
                            logger.debug(f"AddFontResourceExW returned 0 for: {font_path}")

            logger.info(f"Loaded {count} private session fonts from {self.fonts_dir}")
            return count

    def unload_all(self):
        with self._lock:
            for font_path in self.loaded_fonts:
                try:
                    gdi32.RemoveFontResourceExW(font_path, FR_PRIVATE, None)
                except Exception:
                    pass
            self.loaded_fonts.clear()


# ---------------------------------------------------------------------------
# Conversion Result Container
# ---------------------------------------------------------------------------

@dataclass
class ConversionResult:
    wmf_bytes: bytes
    baseline_offset_pt: float
    width_pt: float
    height_pt: float
    method: str
    code: int = 0
    error_message: Optional[str] = None


# ---------------------------------------------------------------------------
# MathType Engine Core
# ---------------------------------------------------------------------------

class MathTypeConverter:
    """High-performance, fault-tolerant MathType binary to WMF converter."""

    def __init__(self, core_dir: Optional[str] = None):
        self.core_dir = self._resolve_core_dir(core_dir)
        self.fonts_dir = os.path.join(self.core_dir, "Fonts")
        self.dll_path = os.path.join(self.core_dir, "MT6.dll")

        if not os.path.isfile(self.dll_path):
            raise FileNotFoundError(f"MT6.dll not found in core dir: {self.core_dir}")

        # Initialize Fonts
        self.font_manager = FontManager(self.fonts_dir)
        self.font_manager.load_all()

        # Load DLL & bind prototypes
        self._init_dll()

        # Clipboard format for MathType EF
        self.cf_mathtype_ef = user32.RegisterClipboardFormatW("MathType EF")

        # Synchronization & state
        self._lock = threading.Lock()
        self._connected = False

        # Dedicated STA worker thread for thread-affine MathType execution
        self._work_queue: queue.Queue = queue.Queue()
        self._worker_thread = threading.Thread(
            target=self._worker_loop,
            name="MathType-Worker",
            daemon=True,
        )
        self._worker_thread.start()

        atexit.register(self.shutdown)

    @staticmethod
    def _resolve_core_dir(custom_path: Optional[str] = None) -> str:
        candidates = []
        if custom_path:
            candidates.append(custom_path)

        env_dir = os.environ.get("MATHTYPE_CORE_DIR")
        if env_dir:
            candidates.append(env_dir)

        # Check PyInstaller bundle locations
        if getattr(sys, "frozen", False):
            base_exe_dir = os.path.dirname(sys.executable)
            meipass = getattr(sys, "_MEIPASS", "")
            candidates.extend([
                os.path.join(base_exe_dir, "mathtype_core"),
                base_exe_dir,
                os.path.join(meipass, "mathtype_core") if meipass else "",
                meipass,
            ])

        # Local application directory / adjacent mathtype_core
        cwd = os.getcwd()
        candidates.extend([
            os.path.join(cwd, "mathtype_core"),
            os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "mathtype_core"),
            r"C:\Program Files (x86)\MathType\System\64",
            r"C:\Program Files (x86)\MathType",
        ])

        for c in candidates:
            p = os.path.abspath(c)
            if os.path.isfile(os.path.join(p, "MT6.dll")):
                return p
            # If 64 subdir exists
            if os.path.isfile(os.path.join(p, "64", "MT6.dll")):
                return os.path.join(p, "64")

        raise FileNotFoundError(
            f"Could not locate mathtype_core containing MT6.dll. Looked in: {candidates}"
        )

    def _init_dll(self):
        """Set DLL directory and configure ctypes restypes for 64-bit MT6.dll."""
        kernel32.SetDllDirectoryW(self.core_dir)
        self.mt6 = ctypes.WinDLL(self.dll_path)

        self.mt6.MTAPIConnect.restype = ctypes.c_int
        self.mt6.MTAPIDisconnect.restype = ctypes.c_int
        self.mt6.MTXFormReset.restype = ctypes.c_int
        self.mt6.MTXFormSetPrefs.restype = ctypes.c_int
        self.mt6.MTXFormGetStatus.restype = ctypes.c_int
        self.mt6.MTXFormEqn.restype = ctypes.c_int
        self.mt6.MTGetLastDimension.restype = ctypes.c_int

    def _ensure_connected(self, force_restart: bool = False):
        """Ensure connection to MathType session; auto-recover if stuck."""
        if self._connected and not force_restart:
            return

        if force_restart:
            self._force_cleanup_session()

        rc = self.mt6.MTAPIConnect(MT_INIT_LAUNCH_AS_NEEDED, 30)
        if rc != MT_OK:
            logger.warning(f"MTAPIConnect failed with: {describe_status(rc)}. Attempting recovery...")
            self._force_cleanup_session()
            rc = self.mt6.MTAPIConnect(MT_INIT_LAUNCH_NOW, 45)
            if rc != MT_OK:
                raise RuntimeError(f"Failed to connect to MathType server: {describe_status(rc)}")

        self._connected = True

    def _force_cleanup_session(self):
        """Kill any zombie MathType.exe processes and disconnect session."""
        try:
            self.mt6.MTAPIDisconnect()
        except Exception:
            pass
        self._connected = False

        # Terminate any orphaned MathType.exe process
        try:
            subprocess.run(
                ["taskkill", "/F", "/IM", "MathType.exe"],
                capture_output=True,
                check=False,
            )
            time.sleep(0.3)
        except Exception as e:
            logger.debug(f"Taskkill exception: {e}")

    def _worker_loop(self):
        """Dedicated STA worker loop ensuring all MT6.dll calls stay on the exact same thread."""
        hr = ole32.CoInitializeEx(None, COINIT_APARTMENTTHREADED)
        try:
            while True:
                item = self._work_queue.get()
                if item is None:
                    break
                func, args, kwargs, future = item
                try:
                    res = func(*args, **kwargs)
                    future.set_result(res)
                except Exception as ex:
                    future.set_exception(ex)
                finally:
                    self._work_queue.task_done()
        finally:
            self._force_cleanup_session()
            if hr in (0, 1):
                ole32.CoUninitialize()

    def shutdown(self):
        """Shutdown converter, stop worker thread, and clean up resources."""
        if hasattr(self, "_worker_thread") and self._worker_thread.is_alive():
            self._work_queue.put(None)
            self._worker_thread.join(timeout=2.0)
        with self._lock:
            if self._connected:
                try:
                    self.mt6.MTAPIDisconnect()
                except Exception:
                    pass
                self._connected = False
            self.font_manager.unload_all()

    # -----------------------------------------------------------------------
    # Clipboard Helpers
    # -----------------------------------------------------------------------

    def _set_clipboard_mtef(self, mtef: bytes) -> bool:
        """Write raw MTEF bytes to the Windows clipboard under 'MathType EF'."""
        if not user32.OpenClipboard(None):
            return False
        try:
            user32.EmptyClipboard()
            h_mem = kernel32.GlobalAlloc(GHND, len(mtef))
            if not h_mem:
                return False
            p_mem = kernel32.GlobalLock(h_mem)
            if not p_mem:
                kernel32.GlobalFree(h_mem)
                return False
            ctypes.memmove(p_mem, mtef, len(mtef))
            kernel32.GlobalUnlock(h_mem)
            return bool(user32.SetClipboardData(self.cf_mathtype_ef, h_mem))
        finally:
            user32.CloseClipboard()

    def _clear_clipboard(self):
        """Empty the Windows Clipboard to release all format handles."""
        if user32.OpenClipboard(None):
            try:
                user32.EmptyClipboard()
            finally:
                user32.CloseClipboard()

    def _read_clipboard_wmf(self) -> Optional[Tuple[bytes, int, int]]:
        """Read CF_METAFILEPICT from Windows Clipboard, returning (raw_records, width_units, height_units)."""
        if not user32.OpenClipboard(None):
            return None
        try:
            h_data = user32.GetClipboardData(CF_METAFILEPICT)
            if not h_data:
                return None
            p_data = kernel32.GlobalLock(h_data)
            if not p_data:
                return None
            try:
                mfp = METAFILEPICT.from_address(p_data)
                width_units = int(mfp.xExt)
                height_units = int(mfp.yExt)

                cb_size = gdi32.GetMetaFileBitsEx(mfp.hMF, 0, None)
                if cb_size == 0:
                    return None
                buf = (ctypes.c_char * cb_size)()
                gdi32.GetMetaFileBitsEx(mfp.hMF, cb_size, ctypes.cast(buf, ctypes.c_void_p))
                return bytes(buf), width_units, height_units
            finally:
                kernel32.GlobalUnlock(h_data)
                user32.EmptyClipboard()
        finally:
            user32.CloseClipboard()

    # -----------------------------------------------------------------------
    # Main Conversion Entry
    # -----------------------------------------------------------------------

    def execute_on_worker(self, func, *args, timeout: float = 30.0, **kwargs):
        """Execute any callable on the dedicated MathType worker thread."""
        if threading.current_thread() is getattr(self, "_worker_thread", None):
            return func(*args, **kwargs)
        future = concurrent.futures.Future()
        self._work_queue.put((func, args, kwargs, future))
        return future.result(timeout=timeout)

    def convert(self, raw_input_bytes: bytes, timeout: float = 30.0) -> ConversionResult:
        """Thread-safe conversion entrypoint routing work to the dedicated MathType STA thread."""
        return self.execute_on_worker(self._convert_internal, raw_input_bytes, timeout=timeout)

    def convert_batch(
        self, items: List[Tuple[str, bytes]], timeout: float = 120.0
    ) -> List[Tuple[str, Optional[ConversionResult], Optional[str]]]:
        """Convert a batch of (id, raw_bytes) on the dedicated STA worker thread in one pass."""
        return self.execute_on_worker(self._convert_batch_internal, items, timeout=timeout)

    def _convert_batch_internal(
        self, items: List[Tuple[str, bytes]]
    ) -> List[Tuple[str, Optional[ConversionResult], Optional[str]]]:
        results = []
        for item_id, raw_bytes in items:
            try:
                res = self._convert_internal(raw_bytes)
                results.append((item_id, res, None))
            except Exception as e:
                results.append((item_id, None, str(e)))
        return results

    def _convert_internal(self, raw_input_bytes: bytes) -> ConversionResult:
        """Convert MathType binary/MTEF bytes to WMF with baseline offset.

        Applies:
        1. Auto MTEF extraction (CFB / Equation Native / Raw).
        2. Primary direct transformation (MtxfmLocal -> MtxfmFile).
        3. Fallback 1: Clipboard Input -> File Output.
        4. Fallback 2: Clipboard Input -> Clipboard PICT Output -> GDI Placeable synthesis.
        """
        mtef = extract_mtef_from_bytes(raw_input_bytes)

        last_direct_err = None
        last_clip1_err = None

        with self._lock:
            # Primary Conversion Attempt
            try:
                return self._convert_direct_local(mtef)
            except Exception as ex_direct:
                last_direct_err = str(ex_direct)
                logger.warning(
                    f"Direct MT6.dll transform failed: {ex_direct}. Falling back to OLE Clipboard relay 1..."
                )

            # Fallback 1: Clipboard Input -> File Output
            try:
                return self._convert_clipboard_to_file(mtef)
            except Exception as ex_clip1:
                last_clip1_err = str(ex_clip1)
                logger.warning(
                    f"Clipboard-to-file fallback failed: {ex_clip1}. Falling back to Clipboard-to-PICT relay 2..."
                )

            # Fallback 2: Clipboard Input -> Clipboard Output -> GDI APM Reassembly
            try:
                return self._convert_clipboard_to_clipboard(mtef)
            except Exception as ex_clip2:
                logger.error(f"All conversion methods failed. Last error: {ex_clip2}")
                raise RuntimeError(
                    f"MathType conversion failed across all execution paths. Direct error: {last_direct_err}; Clipboard error: {ex_clip2}"
                ) from ex_clip2

    # -----------------------------------------------------------------------
    # Method 1: Direct Local Transform
    # -----------------------------------------------------------------------

    def _convert_direct_local(self, mtef: bytes) -> ConversionResult:
        self._ensure_connected()

        fd, temp_wmf = tempfile.mkstemp(suffix=".wmf")
        os.close(fd)
        if os.path.exists(temp_wmf):
            os.remove(temp_wmf)

        try:
            self.mt6.MTXFormReset()
            self.mt6.MTXFormSetPrefs(MTXFM_PREF_MT_DEFAULT, None)

            rc = self.mt6.MTXFormEqn(
                MTXFM_LOCAL,
                MTXFM_MTEF,
                mtef,
                len(mtef),
                MTXFM_FILE,
                MTXFM_PICT,
                None,
                0,
                temp_wmf.encode("ansi"),
                None,
            )

            if rc != MT_OK:
                pref_stat = self.mt6.MTXFormGetStatus(-3)
                transl_stat = self.mt6.MTXFormGetStatus(-2)
                raise RuntimeError(
                    f"MTXFormEqn failed: {describe_status(rc)} (prefs={pref_stat}, transl={transl_stat})"
                )

            if not os.path.exists(temp_wmf) or os.path.getsize(temp_wmf) < 22:
                raise RuntimeError("MathType did not write output WMF file or file is empty.")

            # Dimensions from MTGetLastDimension (units are 32nds of a pt)
            w_val = self.mt6.MTGetLastDimension(MTDIM_WIDTH)
            h_val = self.mt6.MTGetLastDimension(MTDIM_HEIGHT)
            b_val = self.mt6.MTGetLastDimension(MTDIM_BASELINE)

            width_pt = round(w_val / 32.0, 4) if w_val > 0 else 0.0
            height_pt = round(h_val / 32.0, 4) if h_val > 0 else 0.0
            baseline_offset_pt = round(b_val / 32.0, 4) if b_val != MT_ERROR else 0.0

            with open(temp_wmf, "rb") as f:
                wmf_bytes = f.read()

            # Cross-verify baseline from WMF internal comment
            wmf_meta = parse_wmf_dimensions_and_baseline(wmf_bytes)
            if wmf_meta.get("baseline_offset_pt") is not None and baseline_offset_pt == 0.0:
                baseline_offset_pt = wmf_meta["baseline_offset_pt"]

            if width_pt == 0.0:
                width_pt = wmf_meta.get("width_pt", 0.0)
            if height_pt == 0.0:
                height_pt = wmf_meta.get("height_pt", 0.0)

            return ConversionResult(
                wmf_bytes=wmf_bytes,
                baseline_offset_pt=baseline_offset_pt,
                width_pt=width_pt,
                height_pt=height_pt,
                method="direct_mt6_dll",
            )
        finally:
            if os.path.exists(temp_wmf):
                try:
                    os.remove(temp_wmf)
                except Exception:
                    pass

    # -----------------------------------------------------------------------
    # Method 2: Clipboard Input -> File Output
    # -----------------------------------------------------------------------

    def _convert_clipboard_to_file(self, mtef: bytes) -> ConversionResult:
        self._ensure_connected()

        if not self._set_clipboard_mtef(mtef):
            raise RuntimeError("Failed to set MathType EF on Windows Clipboard.")

        fd, temp_wmf = tempfile.mkstemp(suffix=".wmf")
        os.close(fd)
        if os.path.exists(temp_wmf):
            os.remove(temp_wmf)

        dummy_buf = (ctypes.c_char * 16)()

        try:
            self.mt6.MTXFormReset()
            self.mt6.MTXFormSetPrefs(MTXFM_PREF_MT_DEFAULT, None)

            rc = self.mt6.MTXFormEqn(
                MTXFM_CLIPBOARD,
                MTXFM_MTEF,
                dummy_buf,
                0,
                MTXFM_FILE,
                MTXFM_PICT,
                None,
                0,
                temp_wmf.encode("ansi"),
                None,
            )

            if rc != MT_OK:
                raise RuntimeError(f"MTXFormEqn(Clipboard->File) failed: {describe_status(rc)}")

            if not os.path.exists(temp_wmf) or os.path.getsize(temp_wmf) < 22:
                raise RuntimeError("MathType did not write WMF in clipboard-to-file mode.")

            b_val = self.mt6.MTGetLastDimension(MTDIM_BASELINE)
            w_val = self.mt6.MTGetLastDimension(MTDIM_WIDTH)
            h_val = self.mt6.MTGetLastDimension(MTDIM_HEIGHT)

            width_pt = round(w_val / 32.0, 4) if w_val > 0 else 0.0
            height_pt = round(h_val / 32.0, 4) if h_val > 0 else 0.0
            baseline_offset_pt = round(b_val / 32.0, 4) if b_val != MT_ERROR else 0.0

            with open(temp_wmf, "rb") as f:
                wmf_bytes = f.read()

            wmf_meta = parse_wmf_dimensions_and_baseline(wmf_bytes)
            if wmf_meta.get("baseline_offset_pt") is not None and baseline_offset_pt == 0.0:
                baseline_offset_pt = wmf_meta["baseline_offset_pt"]

            return ConversionResult(
                wmf_bytes=wmf_bytes,
                baseline_offset_pt=baseline_offset_pt,
                width_pt=width_pt or wmf_meta.get("width_pt", 0.0),
                height_pt=height_pt or wmf_meta.get("height_pt", 0.0),
                method="clipboard_relay_to_file",
            )
        finally:
            self._clear_clipboard()
            if os.path.exists(temp_wmf):
                try:
                    os.remove(temp_wmf)
                except Exception:
                    pass

    # -----------------------------------------------------------------------
    # Method 3: Clipboard Input -> Clipboard Output -> GDI Assembly
    # -----------------------------------------------------------------------

    def _convert_clipboard_to_clipboard(self, mtef: bytes) -> ConversionResult:
        self._ensure_connected()

        try:
            if not self._set_clipboard_mtef(mtef):
                raise RuntimeError("Failed to set MathType EF on clipboard.")

            dummy_buf = (ctypes.c_char * 1024)()

            self.mt6.MTXFormReset()
            self.mt6.MTXFormSetPrefs(MTXFM_PREF_MT_DEFAULT, None)

            rc = self.mt6.MTXFormEqn(
                MTXFM_CLIPBOARD,
                MTXFM_MTEF,
                dummy_buf,
                0,
                MTXFM_CLIPBOARD,
                MTXFM_PICT,
                dummy_buf,
                1024,
                b"",
                None,
            )

            if rc != MT_OK:
                raise RuntimeError(f"MTXFormEqn(Clipboard->Clipboard) failed: {describe_status(rc)}")

            clip_data = self._read_clipboard_wmf()
            if not clip_data:
                raise RuntimeError("Could not retrieve CF_METAFILEPICT from Windows Clipboard.")

            raw_records, width_units, height_units = clip_data

            b_val = self.mt6.MTGetLastDimension(MTDIM_BASELINE)
            w_val = self.mt6.MTGetLastDimension(MTDIM_WIDTH)
            h_val = self.mt6.MTGetLastDimension(MTDIM_HEIGHT)

            # Reconstruct standard Placeable WMF (APM header + records)
            placeable_wmf = create_placeable_wmf(
                raw_wmf_records=raw_records,
                width_units=w_val if w_val > 0 else width_units,
                height_units=h_val if h_val > 0 else height_units,
                dpi=2304,
            )

            baseline_offset_pt = round(b_val / 32.0, 4) if b_val != MT_ERROR else 0.0
            wmf_meta = parse_wmf_dimensions_and_baseline(placeable_wmf)

            if wmf_meta.get("baseline_offset_pt") is not None and baseline_offset_pt == 0.0:
                baseline_offset_pt = wmf_meta["baseline_offset_pt"]

            width_pt = round(w_val / 32.0, 4) if w_val > 0 else wmf_meta.get("width_pt", 0.0)
            height_pt = round(h_val / 32.0, 4) if h_val > 0 else wmf_meta.get("height_pt", 0.0)

            return ConversionResult(
                wmf_bytes=placeable_wmf,
                baseline_offset_pt=baseline_offset_pt,
                width_pt=width_pt,
                height_pt=height_pt,
                method="clipboard_relay_to_clipboard",
            )
        finally:
            self._clear_clipboard()
