use std::path::PathBuf;
use aios_core::get_db_option;
use crate::io::PdmsIO;



#[test]
pub fn test_get_max_att_pgno() {
    let db_option = get_db_option();
    let dir = db_option.get_project_path(&db_option.project_name).unwrap();
    // let input: PathBuf = format!("{}/{}", &dir, "ams7351_0001").into();
    // dbg!(&input);
    // let output: PathBuf = format!("{}/{}.cba", &dir, "test7351").into();
    // dbg!(&output);

    // let db_path: PathBuf = format!("{}/{}", dir, "ams1112_0001").into();
    dbg!(&dir);
    // let db_path = dir.join("/ams000/ams1112_0001");
    // dbg!(&db_path);
    let db_path = "D:/AVEVA/Projects/E3D2.1/AvevaMarineSample/ams000/ams1112_0001";
    let mut io = PdmsIO::new(db_path, true);
    let max_att_version = io.get_att_latest_pgno().unwrap();
    dbg!(max_att_version);
}