use aios_core::RefU64;
use crate::io::PdmsIO;

#[tokio::test]
async fn test_parse_ele(){
    let refno: RefU64 = "17496/269393".into();
    //首先要根据参考号的索引结构找到这个数据
    // let refno_loc =
    let db_path = "D:/AVEVA/Projects/E3D2.1/AvevaMarineSample/ams000/ams1112_0001";
    let mut io = PdmsIO::new(db_path, true);
    let loc = io.search_refno_pgno(refno).unwrap();
    dbg!(loc.get_att_offset());
    let att = io.get_element(loc.get_att_offset()).await;
    dbg!(att);


}