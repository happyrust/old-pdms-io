@echo off
rem 测试增量元素收集功能的脚本

rem 默认数据库路径，可以根据实际情况修改
set DB_PATH=D:\AVEVA\Projects\E3D2.1\AvevaMarineSample\ams000\ams1112_0001

rem 如果提供了命令行参数，则使用命令行参数作为数据库路径
if not "%~1"=="" set DB_PATH=%~1

rem 如果提供了第二个参数，则使用它作为起始会话号
set START_SESNO=
if not "%~2"=="" set START_SESNO=%~2

rem 如果提供了第三个参数，则使用它作为结束会话号
set END_SESNO=
if not "%~3"=="" set END_SESNO=%~3

echo 数据库路径: %DB_PATH%
echo 起始会话号: %START_SESNO%
echo 结束会话号: %END_SESNO%

rem 构建命令行
set CMD=cargo run --bin test_increment_eles -- "%DB_PATH%"
if not "%START_SESNO%"=="" set CMD=%CMD% %START_SESNO%
if not "%END_SESNO%"=="" set CMD=%CMD% %END_SESNO%

echo 运行命令: %CMD%
%CMD%

echo 测试完成

rem 按任意键继续
pause 