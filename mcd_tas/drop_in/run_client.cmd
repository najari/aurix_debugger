@echo off
rem mcd_basic_client + mcd_tas.dll (drop-in as mcdxdas.dll); DLL log -> client_stderr.log
cd /d %~dp0
set MCD_TAS_LOG=1
mcd_basic_client.exe 2> client_stderr.log
