"""Asks Infineon's MCDS recorder library (DAS64, mcds_recorder.dll) what trace capability the connected adapter has.

Read only: mcdsrl_lib_init, mcdsrl_qry_devices, mcdsrl_connect_device, mcdsrl_device_connected,
mcdsrl_disconnect_device. Nothing is written to the device and no recording is started.
"""
import ctypes
import os
import sys

DLL_DIR = r"C:\Program Files\DAS64\clients"
os.add_dll_directory(DLL_DIR)
lib = ctypes.CDLL(os.path.join(DLL_DIR, "mcds_recorder.dll"))

CPLTY = {0: "UNKNOWN", 1: "NO_TRACE", 4: "ONCHIP_TRACE_ONLY", 8: "CONT_TRACE_UNRELIABLE", 12: "CONT_TRACE_RELIABLE"}

lib.mcdsrl_get_error.restype = ctypes.c_char_p
lib.mcdsrl_device_connected.restype = ctypes.c_void_p
lib.mcdsrl_lib_version.restype = ctypes.c_void_p


def check(name, code):
    print(f"{name}: return code {code}")
    return code


print("init:", end=" ")
check("mcdsrl_lib_init(3)", lib.mcdsrl_lib_init(ctypes.c_uint8(3)))

num = ctypes.c_uint32(0)
devices = ctypes.create_string_buffer(8192)
code = lib.mcdsrl_qry_devices(None, ctypes.c_uint32(0), ctypes.byref(num), None)
check("mcdsrl_qry_devices (count)", code)
print("devices found:", num.value)
if num.value:
    # mcd_core_con_info_st is well under 1 KiB; ask for as many as the buffer holds.
    count = ctypes.c_uint32(0)
    code = lib.mcdsrl_qry_devices(None, ctypes.c_uint32(min(num.value, 8)), ctypes.byref(count), devices)
    check("mcdsrl_qry_devices (list)", code)
    raw = devices.raw
    print("first bytes:", raw[:400].replace(b"\x00", b" ").decode("ascii", "replace").strip())

con = ctypes.create_string_buffer(8192)
code = check("mcdsrl_connect_device(NULL)", lib.mcdsrl_connect_device(None, con))
if code != 0:
    print("last error:", (lib.mcdsrl_get_error() or b"").decode("latin1"))
    sys.exit(1)
try:
    class Cplty(ctypes.Structure):
        _fields_ = [("cplty", ctypes.c_int32), ("bandwidth_kb", ctypes.c_uint32), ("trace_buf_kb", ctypes.c_uint32)]

    cplty = Cplty()
    info = lib.mcdsrl_device_connected(ctypes.byref(cplty))
    print("device connected:", bool(info))
    if info:
        buffer = ctypes.string_at(info, 1024)
        strings = [s.decode("latin1") for s in buffer.split(b"\x00") if len(s) > 2]
        print("connection info strings:", strings[:12])
    print(
        f"capability: {CPLTY.get(cplty.cplty, cplty.cplty)} ({cplty.cplty}), "
        f"continuous trace bandwidth ~{cplty.bandwidth_kb} kB/s, guaranteed on-chip trace buffer {cplty.trace_buf_kb} kB"
    )
finally:
    check("mcdsrl_disconnect_device", lib.mcdsrl_disconnect_device())
