#!/bin/zsh
# Builds and runs strait-win on the Windows PC over SSH, from the Mac.
#
#   scripts/win.sh build    copy the workspace to the PC and build it there
#   scripts/win.sh run      build, then restart strait in the logged-in session
#   scripts/win.sh install  build, then register strait to start at logon
#   scripts/win.sh log      print strait's log
#
# STRAIT_PC names the PC (default: uael@192.168.1.23).
set -euo pipefail

pc=${STRAIT_PC:-uael@192.168.1.23}
root=${0:A:h:h}
# PowerShell on the PC; refresh PATH so tools installed after sshd started are found.
ps() { ssh -o BatchMode=yes "$pc" "\$env:Path = [Environment]::GetEnvironmentVariable('Path','Machine') + ';' + [Environment]::GetEnvironmentVariable('Path','User'); \$ErrorActionPreference = 'Stop'; $1"; }

build() {
  local archive=${TMPDIR:-/tmp}/strait-src.tar
  tar -C "$root" --exclude ./target --exclude ./.git --exclude ./mac/.build -cf "$archive" .
  scp -q "$archive" "${pc}:strait-src.tar"
  rm -f "$archive"
  ps "New-Item -ItemType Directory -Force strait | Out-Null; tar -xf strait-src.tar -C strait; Remove-Item strait-src.tar; Set-Location strait; cmd /c \"cargo build --release -p strait-win 2>&1\"; if (\$LASTEXITCODE) { exit \$LASTEXITCODE }"
}

stop() {
  ps "Get-Process strait-win -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep -Milliseconds 500; New-Item -ItemType Directory -Force bin | Out-Null; Copy-Item strait\\target\\release\\strait-win.exe bin\\strait-win.exe -Force"
}

case ${1:-} in
  build) build ;;
  run)
    build
    stop
    # A task launched with /IT runs on the logged-in desktop, where injected input lands.
    ps "schtasks /Create /F /TN strait-dev /TR \"\$HOME\\bin\\strait-win.exe\" /SC ONCE /ST 23:59 /RU \$env:USERNAME /IT /RL HIGHEST | Out-Null; schtasks /Run /TN strait-dev | Out-Null; Start-Sleep 2; Get-Process strait-win | Select-Object Id,SessionId"
    ;;
  install)
    build
    stop
    ps "cmd /c \"schtasks /Delete /F /TN strait-dev >nul 2>&1\"; cmd /c \"%USERPROFILE%\\bin\\strait-win.exe --install 2>&1\"; exit \$LASTEXITCODE"
    ;;
  log) ps "Get-Content \$env:APPDATA\\strait\\strait.log" ;;
  *) echo "usage: $0 build|run|install|log" >&2; exit 2 ;;
esac
