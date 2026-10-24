@echo off
rem Get the latest ECADCraft changes and run it (first build ~10-15 min, later ones 1-2 min).
cd /d "%~dp0"
git pull --rebase origin feature/iec-device-tagging
cargo run --release -p cadcraft
pause
