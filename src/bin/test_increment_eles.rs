//! 测试增量元素收集功能的可执行程序
//!
//! 用法: cargo run --bin test_increment_eles [数据库路径] [参考号]
//!
//! 如果不传入参数，则使用默认的数据库路径和参考号

use aios_core::get_db_option;
use aios_core::pdms_types::EleOperation;
use aios_core::RefU64;
use pdms_io::io::{EleOperationDetail, PdmsIO};
use std::path::Path;
use std::time::Instant;
// use aios_core::NamedAttrValue;
use aios_core::init_test_surreal; // 导入初始化SurrealDB的函数
use aios_core::SUL_DB; // 导入SurrealDB全局连接
use std::collections::HashMap;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let db_option = get_db_option();
    dbg!(&db_option.get_version_db_conn_str());
    // 初始化SurrealDB连接
    init_test_surreal().await.unwrap();

    // 默认的数据库路径和参考号
    let db_path = std::env::args().nth(1).unwrap_or_else(|| {
        r#"D:\AVEVA\Projects\E3D2.1\AvevaMarineSample\ams000\ams1112_0001"#.to_string()
    });
    let refno_str = std::env::args()
        .nth(2)
        .unwrap_or_else(|| "17496/497128".to_string());

    let project_name = Path::new(&db_path)
        .file_name()
        .and_then(|n| n.to_str())
        .map(|s| s[0..3].to_string())
        .unwrap_or_else(|| "ams".to_string());

    println!("打开数据库: {}", db_path);
    println!("测试参考号: {}", &refno_str);

    // 初始化PDMS IO
    let mut io = PdmsIO::new(project_name.clone(), db_path, true);
    io.open()?;

    // 获取最新会话号
    let latest_sesno = io.get_latest_sesno()? as i32;
    println!("数据库最新会话号: {}", latest_sesno);

    // 测试用例1: 使用None获取最新会话
    println!("\n测试1: 获取最新会话的元素");
    let start_time = Instant::now();
    let max_sesno = io.get_latest_att_pgno()? as i32;
    dbg!(max_sesno);
    // let latest_eles = io.collect_increment_eles(Some(0..=max_sesno)).unwrap();
    // let elapsed = start_time.elapsed();

    // println!("最新会话中共有 {} 个元素, 耗时: {:?}", latest_eles.len(), elapsed);

    // 测试用例2: 使用固定范围
    let range_start = std::cmp::max(1, latest_sesno - 4);
    let sesno_range = range_start..=latest_sesno;

    println!("\n测试2: 获取会话范围 {:?} 内的元素", sesno_range);
    let start_time = Instant::now();
    let range_eles = io.collect_increment_eles(Some(sesno_range.clone()))?;
    let elapsed = start_time.elapsed();

    // 计算所有会话中元素的总数
    let total_elements: usize = range_eles.values().map(|vec| vec.len()).sum();

    println!(
        "会话范围 {:?} 内共有 {} 个元素, 耗时: {:?}",
        sesno_range, total_elements, elapsed
    );

    // 显示前10个元素的详细信息
    println!("\n前10个元素的详细信息:");
    let mut element_count = 0;
    'outer: for (sesno, elements) in &range_eles {
        for element in elements {
            let refno = element.refno;
            let op_type = match &element.detail {
                EleOperationDetail::Add(_) => "新增",
                EleOperationDetail::Modified { .. } => "修改",
                EleOperationDetail::Deleted(_) => "删除",
                EleOperationDetail::None => "无操作",
            };

            println!(
                "{}. 会话={}, 参考号={}, 操作={}, {:?}",
                element_count + 1,
                sesno,
                refno,
                op_type,
                &element.detail
            );
            println!("sql : {}", element.to_surql(&refno.to_string()));

            element_count += 1;
            if element_count >= 10 {
                break 'outer;
            }
        }
    }

    // 将元素操作保存到SurrealDB
    println!("\n将元素操作保存到SurrealDB...");
    let start_time = Instant::now();

    // 创建会话信息表（如果不存在）
    let create_session_table_sql = r#"
    DEFINE TABLE sessions SCHEMAFULL;
    DEFINE FIELD sesno ON sessions TYPE int;
    DEFINE FIELD timestamp ON sessions TYPE datetime;
    DEFINE FIELD dbnum ON sessions TYPE int;
    DEFINE FIELD add_count ON sessions TYPE int;
    DEFINE FIELD modify_count ON sessions TYPE int;
    DEFINE FIELD delete_count ON sessions TYPE int;
    DEFINE FIELD computer_name ON sessions TYPE string;
    DEFINE FIELD comments ON sessions TYPE string;
    DEFINE FIELD end_pgno ON sessions TYPE int;
    DEFINE FIELD index_root_pageno ON sessions TYPE int;
    DEFINE FIELD claim_pageno ON sessions TYPE int;
    "#;

    // 忽略错误，表可能已经存在
    let _ = SUL_DB.query(create_session_table_sql).await;

    // 创建变更记录表（如果不存在）
    // 不能添加schema
    // let create_element_changes_table_sql = r#"
    // DEFINE TABLE element_changes SCHEMAFULL;
    // DEFINE FIELD refno ON element_changes TYPE string;
    // DEFINE FIELD operation_type ON element_changes TYPE string;
    // DEFINE FIELD entity_type ON element_changes TYPE string;
    // DEFINE FIELD timestamp ON element_changes TYPE datetime;
    // DEFINE FIELD session_id ON element_changes TYPE record;
    // DEFINE FIELD sesno ON element_changes TYPE int;
    // DEFINE FIELD details ON element_changes TYPE array<object>;
    // "#;

    // 忽略错误，表可能已经存在
    // let _ = SUL_DB.query(create_element_changes_table_sql).await;

    // 从IO中读取所有会话数据并保存到数据库
    let pdms_header = io.read_pdms_header()?;
    let dbnum = pdms_header.db_num;
    println!("\n1. 先创建所有会话记录...");

    // 获取所有会话号
    let all_sesnos: Vec<u32> = range_eles.keys().cloned().collect();
    println!("找到 {} 个会话", all_sesnos.len());

    // 使用批量插入来创建所有会话记录
    let mut session_records = Vec::new();

    for &sesno in &all_sesnos {
        // 从io获取SessionPageData，包含完整会话信息
        let ses_data = io.get_ses_data(sesno)?;

        let session_record = format!(
            r#"{{
                id: "{}_{}",
                sesno: {},
                timestamp: d"{}",
                dbnum: {},
                add_count: 0,
                modify_count: 0,
                delete_count: 0,
                computer_name: "{}",
                comments: "{}",
                end_pgno: {},
                index_root_pageno: {},
                claim_pageno: {}
            }}"#,
            dbnum,
            sesno,
            sesno,
            ses_data.get_dt().to_rfc3339(),
            dbnum,
            ses_data.get_computer_name(),
            ses_data.get_comments_name(),
            ses_data.end_pgno,
            ses_data.index_root_pageno,
            ses_data.claim_pageno
        );

        session_records.push(session_record);
    }

    // 构建批量插入SQL并执行
    println!("按每批100条记录执行批量插入...");

    // 将记录分批处理，每批最多100条
    for chunk in session_records.chunks(100) {
        // 构建批量插入SQL
        let batch_insert_sql = format!(
            r#"
            INSERT IGNORE INTO sessions [
                {}
            ];
            "#,
            chunk.join(",\n            ")
        );

        // println!("{}", batch_insert_sql);

        // 执行批量插入SQL
        if let Err(e) = SUL_DB.query(&batch_insert_sql).await {
            eprintln!("批量保存会话信息错误: {}", e);
        }
    }

    println!("所有会话数据创建完成");

    // 统计每个会话的操作类型数量
    println!("\n2. 统计每个会话的增删改数量...");
    let mut session_stats: HashMap<i32, (i32, i32, i32)> = HashMap::new();

    // 遍历所有会话和元素
    for (sesno, elements) in &range_eles {
        for element in elements {
            let stats = session_stats.entry(*sesno as i32).or_insert((0, 0, 0));
            match &element.detail {
                EleOperationDetail::Add(_) => stats.0 += 1,
                EleOperationDetail::Modified { .. } => stats.1 += 1,
                EleOperationDetail::Deleted(_) => stats.2 += 1,
                EleOperationDetail::None => {}
            }
        }
    }

    // 更新会话的增删改数量
    println!("\n3. 更新会话的增删改数量...");
    for (sesno, stats) in &session_stats {
        let update_session_sql = format!(
            r#"
            UPDATE sessions:{}_{}
            SET 
                add_count = {},
                modify_count = {},
                delete_count = {}
            ;
            "#,
            dbnum, sesno, stats.0, stats.1, stats.2
        );

        // 执行SQL
        if let Err(e) = SUL_DB.query(&update_session_sql).await {
            eprintln!("更新会话信息错误: {}", e);
        }
    }

    println!("\n4. 保存元素变更记录...");

    // 准备批量插入元素变更记录
    let mut element_records = Vec::new();

    // 遍历所有会话和元素
    for (&sesno, elements) in &range_eles {
        let timestamp = io.get_ses_data(sesno)?.get_dt().to_rfc3339();
        for element in elements {
            let refno = element.refno;
            // 生成SurrealQL语句，更新到最新的数据到数据库，需要比较sesno
            // let surql = element.to_surql(&refno.to_string());
            // if surql.is_empty() {
            //     continue;
            // }
            // 执行SurrealQL语句
            // let _ = SUL_DB.query(&surql).await;

            // 记录变更历史
            let op_type = element.get_op_type();
            let entity_type = match &element.detail {
                EleOperationDetail::Add(ele_data) => ele_data.att_map().get_type(),
                EleOperationDetail::Modified(modified) => modified.noun.clone(),
                EleOperationDetail::Deleted(noun_type) => noun_type.clone(),
                EleOperationDetail::None => "unknown".to_string(),
            };
            let details = if let EleOperationDetail::Modified(modified) = &element.detail {
                modified.to_patch_json()
            } else {
                "[]".to_string()
            };

            // 创建元素变更记录对象
            let element_record = format!(
                r#"{{
                        id: ["{}",{}],
                        refno: "{}",
                        operation_type: "{}",
                        entity_type: "{}",
                        timestamp: d"{}",
                        session_id: sessions:{}_{},
                        sesno: {},
                        details: {}
                    }}"#,
                refno.to_string(),
                sesno,
                refno.to_string(),
                op_type,
                entity_type,
                &timestamp,
                dbnum,
                sesno,
                sesno,
                details
            );

            element_records.push(element_record);
        }
    }

    // 按每批100条记录执行批量插入
    for chunk in element_records.chunks(100) {
        if chunk.len() > 0 {
            // 构建批量插入SQL
            let batch_insert_sql = format!(
                r#"
            INSERT IGNORE INTO element_changes [
                {}
            ];
            "#,
                chunk.join(",\n                ")
            );
            // println!("{}", batch_insert_sql);
            if let Err(e) = SUL_DB.query(&batch_insert_sql).await {
                println!("批量保存元素变更记录错误: {}", e);
            }
        }
    }

    let elapsed = start_time.elapsed();
    println!("保存到SurrealDB完成, 耗时: {:?}", elapsed);

    // 测试用例3: 测试指定参考号
    let refno: RefU64 = refno_str.into();
    println!("\n测试3: 检查指定参考号 {} 的状态", refno);

    let status_map = io.get_refno_operation_status(refno, None)?;

    // 获取指定参考号的操作状态
    if let Some(operation) = status_map.get(&refno) {
        println!("\n参考号 {} 的操作状态: {:?}", refno, operation);
    } else {
        println!("\n找不到参考号 {} 的操作状态", refno);
    }
    println!("状态映射中包含 {} 个元素", status_map.len());

    // 打印子元素的状态
    if status_map.len() > 1 {
        println!("子元素状态列表:");
        for (child_refno, operation) in status_map.iter() {
            if *child_refno != refno {
                println!(" - 子元素 {} 的状态: {:?}", child_refno, operation);
            }
        }
    }

    Ok(())
}
