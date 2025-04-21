#!/bin/bash
# 测试PDMS数据库参考号状态脚本
# 用法: ./test_refno.sh <数据库路径> [参考号]

DB_PATH=$1
REFNO="17496/171606"

if [ ! -z "$2" ]; then
  REFNO=$2
fi

if [ -z "$DB_PATH" ]; then
  echo "用法: $0 <PDMS数据库文件路径> [参考号]"
  echo "示例: $0 ./your_database.pdms 17496/171606"
  exit 1
fi

echo "运行测试: cargo run --bin test_get_refno_status -- $DB_PATH $REFNO"
cargo run --bin test_get_refno_status -- "$DB_PATH" "$REFNO" 