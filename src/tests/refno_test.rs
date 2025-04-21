use aios_core::pdms_types::{EleOperation, RefU64};
use crate::io::PdmsIO;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use std::env;

// 定义测试专用的辅助函数
fn get_test_db_path() -> PathBuf {
    // 尝试从环境变量获取数据库路径
    if let Ok(path) = env::var("PDMS_TEST_DB_PATH") {
        return PathBuf::from(path);
    }
    
    // 如果环境变量未设置，使用默认测试数据库路径
    // 请根据您的实际情况修改此路径
    PathBuf::from("./test_data/test.pdms")
}

fn format_duration(duration: Duration) -> String {
    if duration.as_millis() > 0 {
        format!("{:.2}ms", duration.as_millis() as f64)
    } else {
        format!("{:.2}μs", duration.as_micros() as f64)
    }
}

/// 测试索引映射表的构建和基本查询功能
#[tokio::test]
async fn test_build_index_map() {
    let db_path = get_test_db_path();
    if !db_path.exists() {
        println!("测试数据库不存在: {:?}，跳过测试", db_path);
        return;
    }
    
    // 创建并打开数据库
    let mut io = PdmsIO::new("test", db_path, true);
    io.open().expect("无法打开测试数据库");
    
    // 构建索引映射表
    let start_time = Instant::now();
    let index_map = io.build_index_map().expect("构建索引映射表失败");
    let build_time = start_time.elapsed();
    
    println!("索引构建完成，耗时: {}，索引大小: {} 项", 
             format_duration(build_time), index_map.len());
    
    // 验证索引映射表非空
    assert!(!index_map.is_empty(), "索引映射表不应为空");
}

/// 测试已知存在的参考号状态查询
#[tokio::test]
async fn test_known_refno_status() {
    // 已知存在的参考号 - 您可以根据实际情况修改
    let known_refno = RefU64::try_from("17496/171606").unwrap();
    
    let db_path = get_test_db_path();
    if !db_path.exists() {
        println!("测试数据库不存在: {:?}，跳过测试", db_path);
        return;
    }
    
    // 创建并打开数据库
    let mut io = PdmsIO::new("test", db_path, true);
    io.open().expect("无法打开测试数据库");
    
    // 构建索引映射表
    let index_map = io.build_index_map().expect("构建索引映射表失败");
    
    // 使用索引映射表查找参考号
    let loc = io.fast_lookup_refno(&known_refno, &index_map);
    assert!(loc.is_some(), "已知参考号在索引中未找到");
    
    let loc = loc.unwrap();
    println!("在索引中找到参考号 {}，位置信息: 页号={}, 偏移={}",
             known_refno, loc.pgno, loc.offset);
    
    // 获取元素数据
    let ele_data = io.fast_get_element(known_refno, &index_map).await.expect("获取元素数据失败");
    println!("元素数据获取成功！子元素数量: {}, 属性类型: {}", 
             ele_data.children.len(), ele_data.att_map().get_type());
    
    // 使用传统方法检查参考号状态
    let trad_start = Instant::now();
    let trad_status = io.get_refno_status(known_refno).await.expect("传统方法状态检查失败");
    let trad_time = trad_start.elapsed();
    
    // 使用优化方法检查参考号状态
    let fast_start = Instant::now();
    let fast_status = io.fast_get_refno_status(known_refno, &index_map).await.expect("优化方法状态检查失败");
    let fast_time = fast_start.elapsed();
    
    // 比较性能和结果
    let speedup = trad_time.as_micros() as f64 / fast_time.as_micros().max(1) as f64;
    println!("\n性能比较:");
    println!("传统方法耗时: {}", format_duration(trad_time));
    println!("优化方法耗时: {}", format_duration(fast_time));
    println!("速度提升: {:.2}倍", speedup);
    
    // 验证两种方法结果一致
    assert_eq!(trad_status, fast_status, "两种方法的结果不一致");
    println!("参考号状态: {:?}", fast_status);
}

/// 测试参考号状态检查的性能基准
#[tokio::test]
async fn benchmark_refno_status_check() {
    // 设置测试参数
    const TEST_COUNT: usize = 10; // 测试次数
    
    let db_path = get_test_db_path();
    if !db_path.exists() {
        println!("测试数据库不存在: {:?}，跳过基准测试", db_path);
        return;
    }
    
    // 创建并打开数据库
    let mut io = PdmsIO::new("benchmark", db_path, true);
    io.open().expect("无法打开测试数据库");
    
    // 构建索引映射表
    let index_map = io.build_index_map().expect("构建索引映射表失败");
    
    // 从索引中选择一些参考号进行测试
    let test_refnos: Vec<RefU64> = index_map
        .keys()
        .take(TEST_COUNT)
        .cloned()
        .collect();
    
    println!("选择了 {} 个参考号进行性能测试", test_refnos.len());
    
    // 基准测试1: 使用传统方法
    let trad_start = Instant::now();
    let mut trad_success = 0;
    
    for refno in &test_refnos {
        if let Ok(_) = io.get_refno_status(*refno).await {
            trad_success += 1;
        }
    }
    
    let trad_time = trad_start.elapsed();
    let trad_avg = trad_time.div_f64(test_refnos.len() as f64);
    
    // 基准测试2: 使用索引映射表优化方法
    let fast_start = Instant::now();
    let mut fast_success = 0;
    
    for refno in &test_refnos {
        if let Ok(_) = io.fast_get_refno_status(*refno, &index_map).await {
            fast_success += 1;
        }
    }
    
    let fast_time = fast_start.elapsed();
    let fast_avg = fast_time.div_f64(test_refnos.len() as f64);
    
    // 计算性能提升
    let speedup = trad_time.as_micros() as f64 / fast_time.as_micros().max(1) as f64;
    
    println!("\n性能基准测试结果:");
    println!("传统方法总耗时: {}, 平均每个: {}, 成功率: {}/{}", 
             format_duration(trad_time), format_duration(trad_avg), 
             trad_success, test_refnos.len());
    println!("优化方法总耗时: {}, 平均每个: {}, 成功率: {}/{}", 
             format_duration(fast_time), format_duration(fast_avg), 
             fast_success, test_refnos.len());
    println!("速度提升: {:.2}倍", speedup);
    
    // 确保两种方法的成功率一致
    assert_eq!(trad_success, fast_success, "两种方法的成功率不一致");
}

/// 测试批量获取元素的性能
#[tokio::test]
async fn test_batch_element_retrieval() {
    // 设置测试参数
    const BATCH_SIZE: usize = 5; // 批量获取的元素数量
    
    let db_path = get_test_db_path();
    if !db_path.exists() {
        println!("测试数据库不存在: {:?}，跳过测试", db_path);
        return;
    }
    
    // 创建并打开数据库
    let mut io = PdmsIO::new("test", db_path, true);
    io.open().expect("无法打开测试数据库");
    
    // 构建索引映射表
    let index_map = io.build_index_map().expect("构建索引映射表失败");
    
    // 从索引中选择一些参考号进行测试
    let test_refnos: Vec<RefU64> = index_map
        .keys()
        .take(BATCH_SIZE)
        .cloned()
        .collect();
    
    // 使用批量获取方法
    let batch_start = Instant::now();
    let batch_elements = io.fast_get_elements(&test_refnos, &index_map).await.expect("批量获取失败");
    let batch_time = batch_start.elapsed();
    
    // 使用单个获取方法
    let individual_start = Instant::now();
    let mut individual_elements = Vec::new();
    
    for refno in &test_refnos {
        if let Ok(ele) = io.auto_get_element(*refno).await {
            individual_elements.push((*refno, ele));
        }
    }
    
    let individual_time = individual_start.elapsed();
    
    // 计算批量获取的性能提升
    let speedup = individual_time.as_micros() as f64 / batch_time.as_micros().max(1) as f64;
    
    println!("\n批量获取性能测试:");
    println!("批量获取耗时: {}, 获取到 {} 个元素", 
             format_duration(batch_time), batch_elements.len());
    println!("传统逐个获取耗时: {}, 获取到 {} 个元素", 
             format_duration(individual_time), individual_elements.len());
    println!("批量获取性能提升: {:.2}倍", speedup);
    
    // 确保获取的元素数量一致
    assert_eq!(batch_elements.len(), individual_elements.len(), "两种方法获取的元素数量不一致");
} 