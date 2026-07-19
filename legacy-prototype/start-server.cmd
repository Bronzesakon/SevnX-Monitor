@echo off
setlocal
cd /d "%~dp0"
"C:\Program Files\nodejs\node.exe" server\index.mjs > server.log 2> server-error.log
