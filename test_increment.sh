#!/bin/bash
# 测试增量元素收集功能的脚本

# 默认数据库路径，可以根据实际情况修改
DB_PATH="D:/AVEVA/Projects/E3D2.1/AvevaMarineSample/ams000/ams1112_0001"

# 如果提供了命令行参数，则使用命令行参数作为数据库路径
if [ ! -z "$1" ]; then
    DB_PATH="$1"
fi

# 如果提供了第二个参数，则使用它作为起始会话号
START_SESNO=""
if [ ! -z "$2" ]; then
    START_SESNO="$2"
fi

# 如果提供了第三个参数，则使用它作为结束会话号
END_SESNO=""
if [ ! -z "$3" ]; then
    END_SESNO="$3"
fi

echo "数据库路径: $DB_PATH"
echo "起始会话号: $START_SESNO"
echo "结束会话号: $END_SESNO"

# 构建命令行
CMD="cargo run --bin test_increment_eles -- \"$DB_PATH\""
if [ ! -z "$START_SESNO" ]; then
    CMD="$CMD $START_SESNO"
fi
if [ ! -z "$END_SESNO" ]; then
    CMD="$CMD $END_SESNO"
fi

echo "运行命令: $CMD"
eval $CMD

echo "测试完成"

# 按任意键继续
read -p "按任意键继续..." key 