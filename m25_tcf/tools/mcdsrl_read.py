"""Reads single words through Infineon's MCDS recorder library (read only).

SCU_CHIPID is the control that must work. The MCDS registers of the TC27x ED (OCS and friends)
are in the EEC address map, which this adapter blocks for probe-rs (error 0x8D): does the
library reach them?
"""
import ctypes
import os

DLL_DIR = r"C:\Program Files\DAS64\clients"
os.add_dll_directory(DLL_DIR)
lib = ctypes.CDLL(os.path.join(DLL_DIR, "mcds_recorder.dll"))
lib.mcdsrl_get_error.restype = ctypes.c_char_p

assert lib.mcdsrl_lib_init(ctypes.c_uint8(3)) == 0
con = ctypes.create_string_buffer(8192)
assert lib.mcdsrl_connect_device(None, con) == 0

NAMES = {0: "SUCCESS", 1: "DEVICE_PORST", 0x10: "DEVICE_DATA (access failed)", 0x100: "DEVICE_CONNECTION", 0x300: "TOOL_HW_CONNECTION",
         0x01000000: "FUNCTION_USED_WRONGLY", 0x10000000: "FUNCTION_NOT_SUPPORTED"}
try:
    for name, address in [
        ("SCU_CHIPID (control)", 0xF0036140),
        ("MCDS OCS, EEC 0xF00E6004", 0xF00E6004),
        ("MCDS OCS as 0xF90E6004", 0xF90E6004),
        ("MCDS ctrl 0xF00E6000", 0xF00E6000),
        ("EMEM 0x9F000000", 0x9F000000),
        ("EMEM 0xAF00FFE0", 0xAF00FFE0),
    ]:
        value = ctypes.c_uint32(0xDEADBEEF)
        code = lib.mcdsrl_read(ctypes.c_uint32(address), ctypes.byref(value))
        error = (lib.mcdsrl_get_error() or b"").decode("latin1") if code else ""
        print(f"{name:<28} {address:#010x}: {NAMES.get(code, hex(code))}  value {value.value:#010x}  {error}")
finally:
    print("disconnect:", lib.mcdsrl_disconnect_device())
