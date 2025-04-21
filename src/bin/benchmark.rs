use anyhow::Result;
use aios_core::pdms_types::RefU64;
use pdms_io::io::PdmsIO;
use rand::seq::SliceRandom;
use std::env;
use std::time::Instant;

#[tokio::main]
async fn main() -> Result<()> {
    // 获取命令行参数
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("用法: {} <PDMS数据库文件路径> [测试数量=100]", args[0]);
        return Ok(());
    }

    let db_path = &args[1];
    let test_count = if args.len() > 2 {
        args[2].parse::<usize>().unwrap_or(100)
    } else {
        100
    };
    
    // 创建并打开数据库
    println!("打开数据库: {}", db_path);
    let mut io = PdmsIO::new("benchmark", db_path, true);
    io.open()?;
    
    // 构建索引映射表
    println!("构建索引映射表...");
    let start_time = Instant::now();
    let index_map = io.build_index_map()?;
    let build_time = start_time.elapsed();
    
    println!("索引构建完成，耗时: {:?}, 索引大小: {} 项", build_time, index_map.len());
    
    // 从索引中随机选择一些参考号进行测试
    println!("从索引中随机选择 {} 个参考号进行测试...", test_count);
    let mut rng = rand::thread_rng();
    let selected_refnos: Vec<RefU64> = index_map
        .keys()
        .cloned()
        .collect::<Vec<_>>()
        .choose_multiple(&mut rng, test_count.min(index_map.len()))
        .cloned()
        .collect();
    
    println!("随机选择了 {} 个参考号", selected_refnos.len());
    
    // 基准测试1: 使用索引映射表查找
    println!("\n基准测试1: 使用索引映射表查找");
    let index_start = Instant::now();
    
    let mut found_count = 0;
    for refno in &selected_refnos {
        if let Some(_) = io.fast_lookup_refno(refno, &index_map) {
            found_count += 1;
        }
    }
    
    let index_time = index_start.elapsed();
    println!("索引映射表查找耗时: {:?}, 找到 {}/{} 个参考号", 
             index_time, found_count, selected_refnos.len());
    
    // 基准测试2: 使用传统search_refno_pgno方法查找
    println!("\n基准测试2: 使用传统search_refno_pgno方法查找");
    let traditional_start = Instant::now();
    
    let mut found_count = 0;
    for refno in &selected_refnos {
        match io.search_refno_pgno(*refno) {
            Ok(_) => found_count += 1,
            Err(_) => {}
        }
    }
    
    let traditional_time = traditional_start.elapsed();
    println!("传统方法查找耗时: {:?}, 找到 {}/{} 个参考号", 
             traditional_time, found_count, selected_refnos.len());
    
    // 计算性能提升
    let speedup = traditional_time.as_micros() as f64 / index_time.as_micros() as f64;
    println!("\n性能提升: {:.2}倍", speedup);
    
    // 基准测试3: 批量获取元素数据
    println!("\n基准测试3: 批量获取元素数据");
    
    // 选择少量参考号进行测试，避免输出过多
    let small_sample = selected_refnos.iter().take(5).cloned().collect::<Vec<_>>();
    
    // 使用索引映射表批量获取
    let batch_start = Instant::now();
    let elements = io.fast_get_elements(&small_sample, &index_map).await?;
    let batch_time = batch_start.elapsed();
    
    println!("批量获取耗时: {:?}, 获取到 {} 个元素", batch_time, elements.len());
    
    // 使用传统方法逐个获取
    let individual_start = Instant::now();
    let mut individual_elements = Vec::new();
    
    for refno in &small_sample {
        match io.auto_get_element(*refno).await {
            Ok(ele) => individual_elements.push((*refno, ele)),
            Err(_) => {}
        }
    }
    
    let individual_time = individual_start.elapsed();
    println!("传统逐个获取耗时: {:?}, 获取到 {} 个元素", 
             individual_time, individual_elements.len());
    
    // 计算批量获取的性能提升
    let batch_speedup = individual_time.as_micros() as f64 / batch_time.as_micros() as f64;
    println!("批量获取性能提升: {:.2}倍", batch_speedup);
    
    println!("\n测试完成!");
    Ok(())
} 