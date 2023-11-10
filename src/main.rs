use std::fs::File;
use std::io::Write;
use std::mem::size_of;
use std::path::PathBuf;
use aios_core::pdms_types::RefU64;
use parse_pdms_db::test_cases::convert_str_to_bytes;
use pdms_io::defines::{ElePageData, EleRawData, PAGE_SIZE};
use pdms_io::io::PdmsIO;
use pdms_io::test::test_data::TEST_DATA;
use pdms_io::watch::PdmsWatcher;


#[test]
fn test_read_eles() -> anyhow::Result<()> {
    // let data = convert_str_to_bytes(TEST_DATA);
    // let mut ele_page = ElePageData::try_from(&data[0..PAGE_SIZE])?;
    // dbg!(ele_page.eles_vec.len());
    // dbg!(ele_page.remain_bytes.len() / 4);
    // println!("Element data: {:#4X?}", ele_page.eles_vec.last().unwrap());

    // let mut watch_files: Vec<PathBuf> = Vec::new();
    // watch_files.push(r#"D:\AVEVA\Projects\E3D2.1\AvevaMarineSample\ams000"#.into());
    let db_filepath = r#"D:\AVEVA\Projects\E3D2.1\AvevaMarineSample\ams000\ams1112_0001"#;
    let mut io = PdmsIO::new(db_filepath.clone(), true);
    io.open()?;
    io.collect_increment_eles(None);
    // io.search_refno(RefU64::from_refno_str("17496/184133").unwrap())?;
    Ok(())
}

pub fn test_write() -> anyhow::Result<()> {
    // let data = convert_str_to_bytes(TEST_MEMEBS_DATA);
    let data = convert_str_to_bytes(TEST_DATA);
    // let d = &data[4..];
    //需要分段
    // let mut offset = 1;
    // let mut header = EleHeaderData::try_from(&d[offset..offset + size_of::<EleHeaderData>()])?;
    // let l = usize::from_be_bytes(d[0..4].try_into().unwrap()) * 4;
    // dbg!(l);
    // let mut ele_vec = vec![];
    // let mut cur_data = &data[4..];
    // while let Ok((rest, mut ele_raw_data)) = EleRawData::from_bytes((&cur_data[..], 0))  {
    //     ele_vec.push(ele_raw_data);
    //     cur_data = rest.0;
    //     dbg!(cur_data.len());
    //     let (_, peek) = u32::read(cur_data, Endian::Big)?;
    //     if cur_data.len() < 7  {
    //         break;
    //     }
    // }
    let mut ele_page = ElePageData::try_from(&data[0..PAGE_SIZE])?;
    dbg!(ele_page.eles_vec.len());
    // dbg!(ele_page.remain_bytes.len() / 4);
    println!("Element data: {:#4X?}", ele_page.eles_vec.last().unwrap());

    let mut out_file = File::create("origin.bin")?;
    let mut bytes: Vec<u8> = ele_page.clone().try_into().unwrap();
    out_file.write_all(&bytes)?;

    // let mut head = EleHeaderData::try_from(head_data)?;
    // println!("head: {:#4X?}",&head);

    let mut io = PdmsIO::new("pdms-test-data/sam7200_0001_back", true);
    let basic_info = io.get_page_basic_info()?;
    // println!("basic info: {:#4X?}",&basic_info);
    let new_ses_no = basic_info.latest_ses_pageno + 1;

    for e in &mut ele_page.eles_vec {
        e.page_no = new_ses_no;
    }

    let mut out_file = File::create("new.bin")?;
    let mut bytes: Vec<u8> = ele_page.try_into().unwrap();
    out_file.write_all(&bytes)?;


    //要找到对应的element page

    //写入到下一个page里，临时实现，后面要考虑细节(不一定是下一个page，后续需要通过索引)
    // head.page_no = basic_info.ses_pagno + 1;

    Ok(())
}


#[tokio::main]
async fn main() -> anyhow::Result<()> {

    // let mut io = PdmsIO::new("D:/AVEVA/Plant/Projects12.1.SP4/Sample/sam7200_0001_back".to_string());

    // io.search_refno(RefU64::from_refno_str("23584/5661").unwrap())?;

    //0x161d
    // let refno = RefU64::from_refno_str("23584/5661").unwrap();
    // test_write(refno, 45.0)?;

    let mut watch_files: Vec<PathBuf> = Vec::new();
    watch_files.push(r#"D:\AVEVA\Projects\E3D2.1\AvevaMarineSample\ams000"#.into());
    //scan_dbs_version(path.clone());
    let mut pdms_watcher = PdmsWatcher::new(watch_files);
    pdms_watcher.init_local_watcher()?;
    // pdms_watcher.async_watch().await?;

    // futures::executor::block_on(async {
    //     if let Err(e) = async_watch(path).await {
    //         println!("error: {:?}", e)
    //     }return;
    // });


    Ok(())
}

