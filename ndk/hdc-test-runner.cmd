@echo off
rem Cargo test runner for aarch64-unknown-linux-ohos (ohos-craft-p2, task 3.5).
rem
rem Host `cargo test -p <crate>` cannot link: the ohos-*-sys crates emit
rem #[link(name=...)] directives for OHOS native libs that have no Windows
rem import-library counterparts. This runner pushes the freshly built test
rem binary to the connected device via hdc, executes it there, streams the
rem libtest output back, and propagates the remote exit code so cargo reports
rem real pass/fail.
rem
rem Adapted from egui-repo ohos-spike (ohos-winit-p1, device-proven); remote
rem and temp names renamed spike_cargo_test -> pdfcraft_cargo_test so
rem concurrent runs from the two repos cannot clobber each other's binary.
rem
rem Invoked by cargo as: hdc-test-runner.cmd <test-binary> [libtest args...]

setlocal enabledelayedexpansion
set "BIN=%~f1"
if not exist "%BIN%" (
    echo hdc-test-runner: test binary not found: %BIN% 1>&2
    exit /b 1
)

rem Forward remaining args (libtest flags/filters) to the remote invocation.
set "REST="
shift
:collect
if not "%~1"=="" (
    set "REST=!REST! %~1"
    shift
    goto collect
)

hdc file send "%BIN%" /data/local/tmp/pdfcraft_cargo_test >nul 2>&1
if errorlevel 1 (
    echo hdc-test-runner: hdc file send failed - is the device online? 1>&2
    exit /b 1
)

set "OUT=%TEMP%\pdfcraft_cargo_test_out.txt"
hdc shell "chmod +x /data/local/tmp/pdfcraft_cargo_test && /data/local/tmp/pdfcraft_cargo_test%REST%; echo REMOTE_EXIT_CODE=$?" > "%OUT%" 2>&1
type "%OUT%"

rem hdc shell does not propagate the remote exit code (always 0) - sniff the
rem marker line instead. findstr errorlevel 1 = marker absent or nonzero.
findstr /B /C:"REMOTE_EXIT_CODE=0" "%OUT%" >nul
if errorlevel 1 (
    echo hdc-test-runner: remote test run failed 1>&2
    exit /b 1
)
exit /b 0
