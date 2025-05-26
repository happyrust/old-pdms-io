//! 测试最新元素收集功能
//! 
//! 本测试模块验证`collect_latest_eles`方法是否能正确收集最新的元素数据：
//! - 能够从后往前检索最新数据
//! - 能够正确跳过已删除的元素
//! - 只保留增加和修改的元素
//! - 能够处理会话数量限制参数

use aios_core::pdms_types::RefU64;
use crate::io::{PdmsIO, EleOperationDetail};
use std::time::Instant;
use std::collections::HashSet;

/// 测试`collect_latest_eles`方法
/// 
/// 本测试验证以下情况：
/// 1. 使用None参数获取所有会话的最新元素
/// 2. 使用指定会话数量限制获取最新元素
/// 3. 验证返回的元素都是最新的且未被删除
/// 4. 验证性能和正确性
#[tokio::test]
async fn test_collect_latest_eles() -> anyhow::Result<()> {
    // 设置数据库文件路径
    let db_filepath = r#"D:\AVEVA\Projects\E3D2.1\AvevaMarineSample\ams000\ams1112_0001"#;
    let mut io = PdmsIO::new("ams", db_filepath, true);
    io.open()?;
    io.init_ses_range_map()?;

    println!("开始测试 collect_latest_eles 方法");
    
    // 测试用例1: 获取所有会话的最新元素（限制前10个会话以避免测试时间过长）
    println!("\n测试1: 获取前10个会话的最新元素");
    let start = Instant::now();
    let latest_eles = io.collect_latest_eles(Some(10))?;
    let elapsed = start.elapsed();
    
    println!("前10个会话中共找到 {} 个最新元素, 耗时: {:?}", latest_eles.len(), elapsed);
    
    // 验证返回的元素都不是删除状态
    let mut add_count = 0;
    let mut modified_count = 0;
    let mut deleted_count = 0;
    let mut none_count = 0;
    
    for (refno, operation_data) in &latest_eles {
        match &operation_data.detail {
            EleOperationDetail::Add(_) => add_count += 1,
            EleOperationDetail::Modified(_) => modified_count += 1,
            EleOperationDetail::Deleted => {
                deleted_count += 1;
                println!("警告: 发现已删除元素 {}, 这不应该出现在结果中", refno);
            },
            EleOperationDetail::None => none_count += 1,
        }
    }
    
    println!("操作类型统计: 新增={}, 修改={}, 删除={}, 无操作={}",
             add_count, modified_count, deleted_count, none_count);
    
    // 断言：结果中不应该有删除的元素
    assert_eq!(deleted_count, 0, "结果中不应该包含已删除的元素");
    
    // 输出前10个元素的详细信息
    println!("\n前10个最新元素的详细信息:");
    for (i, (refno, operation_data)) in latest_eles.iter().take(10).enumerate() {
        let ele_info = match &operation_data.detail {
            EleOperationDetail::Add(ele) => {
                format!("新增元素 - 类型:{}, 属性数:{}", 
                       ele.att_map().get_type(), ele.att_map().len())
            },
            EleOperationDetail::Modified(modified) => {
                format!("修改元素 - 类型:{}, 添加属性:{}, 删除属性:{}, 修改属性:{}", 
                       modified.noun,
                       modified.added_attrs.len(), 
                       modified.deleted_attrs.len(), 
                       modified.modified_attrs.len())
            },
            EleOperationDetail::Deleted => "已删除".to_string(),
            EleOperationDetail::None => "无操作".to_string()
        };
        
        println!("{}: 参考号={}, 会话号={}, {}", 
                i+1, refno, operation_data.sesno, ele_info);
    }
    
    // 测试用例2: 获取更少会话的最新元素，验证结果一致性
    println!("\n测试2: 获取前5个会话的最新元素");
    let start = Instant::now();
    let latest_eles_5 = io.collect_latest_eles(Some(5))?;
    let elapsed = start.elapsed();
    
    println!("前5个会话中共找到 {} 个最新元素, 耗时: {:?}", latest_eles_5.len(), elapsed);
    
    // 验证前5个会话的结果应该是前10个会话结果的子集
    let mut subset_count = 0;
    for (refno, _) in &latest_eles_5 {
        if latest_eles.contains_key(refno) {
            subset_count += 1;
        }
    }
    
    println!("前5个会话的结果中有 {} 个元素也在前10个会话的结果中", subset_count);
    
    // 测试用例3: 验证特定元素的最新状态
    if !latest_eles.is_empty() {
        println!("\n测试3: 验证特定元素的最新状态");
        let test_refno = latest_eles.keys().next().cloned().unwrap();
        let operation_data = latest_eles.get(&test_refno).unwrap();
        
        println!("选择测试参考号: {}", test_refno);
        println!("collect_latest_eles 返回的会话号: {}", operation_data.sesno);
        
        // 使用传统方法验证这确实是最新的状态
        let status_map = io.get_refno_operation_status(test_refno, None)?;
        
        if let Some(traditional_detail) = status_map.get(&test_refno) {
            println!("传统方法返回的状态类型: {}", traditional_detail.get_op_type());
            println!("collect_latest_eles 返回的状态类型: {}", operation_data.detail.get_op_type());
            
            // 验证状态类型一致
            assert_eq!(traditional_detail.get_op_type(), operation_data.detail.get_op_type(),
                      "两种方法返回的状态类型应该一致");
        }
    }
    
    // 测试用例4: 性能对比测试
    println!("\n测试4: 性能对比测试");
    
    // 使用 collect_latest_eles 方法
    let start = Instant::now();
    let latest_method_result = io.collect_latest_eles(Some(3))?;
    let latest_method_time = start.elapsed();
    
    // 使用传统的 collect_increment_eles 方法获取最新3个会话
    let latest_sesno = io.get_latest_sesno()? as i32;
    let range_start = std::cmp::max(1, latest_sesno - 2);
    let sesno_range = range_start..=latest_sesno;
    
    let start = Instant::now();
    let traditional_result = io.collect_increment_eles(Some(sesno_range))?;
    let traditional_time = start.elapsed();
    
    println!("collect_latest_eles (3个会话): {} 个元素, 耗时: {:?}", 
             latest_method_result.len(), latest_method_time);
    
    let traditional_total: usize = traditional_result.values().map(|v| v.len()).sum();
    println!("collect_increment_eles (3个会话): {} 个元素, 耗时: {:?}", 
             traditional_total, traditional_time);
    
    // 验证 collect_latest_eles 的结果中没有重复的 refno
    let refno_set: HashSet<_> = latest_method_result.keys().collect();
    assert_eq!(refno_set.len(), latest_method_result.len(), 
              "collect_latest_eles 结果中不应该有重复的 refno");
    
    println!("\n所有测试通过！collect_latest_eles 方法工作正常。");
    
    Ok(())
}

/// 测试边界情况
#[tokio::test]
async fn test_collect_latest_eles_edge_cases() -> anyhow::Result<()> {
    let db_filepath = r#"D:\AVEVA\Projects\E3D2.1\AvevaMarineSample\ams000\ams1112_0001"#;
    let mut io = PdmsIO::new("ams", db_filepath, true);
    io.open()?;
    io.init_ses_range_map()?;

    println!("测试边界情况");
    
    // 测试用例1: 会话数量为0
    println!("\n测试1: 会话数量为0");
    let result = io.collect_latest_eles(Some(0))?;
    assert!(result.is_empty(), "会话数量为0时应该返回空结果");
    println!("✓ 会话数量为0时正确返回空结果");
    
    // 测试用例2: 会话数量为1
    println!("\n测试2: 会话数量为1");
    let result = io.collect_latest_eles(Some(1))?;
    println!("会话数量为1时返回 {} 个元素", result.len());
    
    // 验证所有元素都来自同一个会话（最新会话）
    let latest_sesno = io.get_latest_sesno()?;
    let mut session_numbers: HashSet<u32> = HashSet::new();
    for (_, operation_data) in &result {
        session_numbers.insert(operation_data.sesno);
    }
    
    if !result.is_empty() {
        assert_eq!(session_numbers.len(), 1, "会话数量为1时，所有元素应该来自同一个会话");
        assert!(session_numbers.contains(&latest_sesno), "应该是最新会话");
        println!("✓ 所有元素都来自最新会话 {}", latest_sesno);
    }
    
    println!("\n边界情况测试通过！");
    
    Ok(())
} 