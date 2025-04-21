@echo off
REM 测试PDMS数据库参考号状态脚本
REM 用法: test_refno.cmd <数据库路径> [参考号]

set DB_PATH=%1
set REFNO=17496/171606

if not "%2"=="" (
  set REFNO=%2
)

if "%DB_PATH%"=="" (
  echo 用法: %0 ^<PDMS数据库文件路径^> [参考号]
  echo 示例: %0 ./your_database.pdms 17496/171606
  exit /b 1
)

echo 运行测试: cargo run --bin test_get_refno_status -- %DB_PATH% %REFNO%
cargo run --bin test_get_refno_status -- %DB_PATH% %REFNO% 