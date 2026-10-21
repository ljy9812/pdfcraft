@echo off
rem OHOS NDK linker wrapper (new NDK versions ship no .cmd wrappers on Windows).
rem The SDK root is centralized HERE: switch SDK by editing this line only.
rem Windows-host OHOS SDK: AppData API 20 (D:\ohos\sdk is the LINUX SDK pack
rem and cannot be used as a Windows linker).
rem Adapted from egui-repo ohos-spike (ohos-winit-p1, device-proven) for
rem ohos-craft-p2 task 3.5.
set "OHOS_NATIVE=C:\Users\65468\AppData\Local\OpenHarmony\Sdk\20\native"
"%OHOS_NATIVE%\llvm\bin\clang.exe" --target=aarch64-unknown-linux-ohos --sysroot="%OHOS_NATIVE%\sysroot" -D__MUSL__ %*
