//两个endpoint 互相传递数据
//两边初始化的时候，本身就会有一次同步更新，先处理这个
//增量更新，每次更新的时候，都会有一个版本号，每次更新的时候，都会更新版本号

use crate::sync::files::sync_e3d_files;
use crate::sync::sync::compress_archive;
use aios_core::get_db_option;
use dpcsync::chunker;
use std::path::PathBuf;
use std::time::Instant;
use crate::sync::compress::{CompressOptions, execute_compress};

#[tokio::test]
pub async fn test_sync_remote_files() {
    sync_e3d_files().await;
}

#[tokio::test]
pub async fn test_gen_cba_file() {
    let db_option = get_db_option();
    let dir = format!(
        "{}/AvevaMarineSample/ams000/",
        db_option.project_path.as_str()
    );
    // let input: PathBuf = format!("{}/{}", &dir, "ams7351_0001").into();
    // dbg!(&input);
    // let output: PathBuf = format!("{}/{}.cba", &dir, "test7351").into();
    // dbg!(&output);

    let input: PathBuf = format!("{}/{}", &dir, "ams1112_0001 copy-backup").into();
    dbg!(&input);
    let file_name = input.file_stem().unwrap().to_str().unwrap();
    dbg!(file_name);
    dbg!(file_name.ends_with("0001"));
    let output: PathBuf = format!("{}/{}.cba", &dir, "test1112").into();
    dbg!(&output);
    let mut time = Instant::now();
    let compress_opt = CompressOptions::new(input, output);
    // dbg!(&compress_opt);
    execute_compress(compress_opt).await.unwrap();
    println!("compress_archive cost: {:?}", time.elapsed().as_secs_f64());
}
