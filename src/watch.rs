use std::fs::File;
use std::io::{Read, Write};
use crate::defines::{DbPageBasicInfo, PdmsHeader};
use crate::io::PdmsIO;
use futures::{
    channel::mpsc::{channel, Receiver},
    SinkExt, StreamExt, future::ok,
};
use indexmap::IndexMap;
use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::Path;
use std::path::PathBuf;
use walkdir::WalkDir;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};

#[test]
fn test_watch() {
    // let path = std::env::args()
    //     .nth(1)
    //     .expect("Argument 1 needs to be a path");
    // println!("watching {}", path);
    // let mut watch_files: Vec<PathBuf> = Vec::new();
    // watch_files.push(r#"D:\AVEVA\Projects\E3D2.1\AvevaMarineSample\ams000"#.into());
    // let path = watch_files[0].clone();
    // //scan_dbs_version(path.clone());

    // futures::executor::block_on(async {
    //     if let Err(e) = async_watch(path).await {
    //         println!("error: {:?}", e)
    //     }return;
    // });
}

///文件的监控
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct PdmsWatcher {
    pub watch_dirs: Vec<PathBuf>,
    pub headers: DashMap<PathBuf, DbPageBasicInfo>,
}

impl PdmsWatcher {
    pub fn new<P: AsRef<Path>>(dirs: Vec<P>) -> Self {
        Self {
            watch_dirs: dirs.into_iter().map(|x| x.as_ref().to_path_buf()).collect(),
            headers: Default::default(),
        }
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let mut file = File::create("watcher.json")?;
        file.write_all(serde_json::to_string(self)?.as_bytes())?;
        Ok(())
    }

    pub fn load_from_json() -> anyhow::Result<Self> {
        let mut file = File::open("watcher.json")?;
        let mut string = String::new();
        file.read_to_string(&mut string)?;
        let w = serde_json::from_str(string.as_str())?;
        Ok(w)
    }

    pub fn init_local_watcher(&self) -> anyhow::Result<()> {
        for watch_dir in &self.watch_dirs {
            for entry in WalkDir::new(watch_dir).sort_by(|a, b| {
                b.path()
                    .metadata()
                    .unwrap()
                    .len()
                    .cmp(&a.path().metadata().unwrap().len())
            }) {
                let dir_entry = entry.unwrap();
                let path = dir_entry.path();
                if path.is_dir() {
                    continue;
                }
                let mut io = PdmsIO::new(path, true);
                io.open()?;
                if let Ok(basic_info) = io.get_page_basic_info() {
                    // let new_ses_no = basic_info.latest_ses_pageno + 1;
                    if let Some(old) = self.headers.get_mut(&path.to_path_buf()) {
                        //未发生修改，直接跳过
                        if old.pdms_header.page_no == basic_info.pdms_header.page_no { continue; }
                    }

                    self.headers.insert(path.to_path_buf(), basic_info);
                }
            }
        }

        anyhow::Ok(())
    }

    // pub async fn async_watch(&self) -> notify::Result<()> {
    //
    //     let (mut watcher, mut rx) = Self::async_watcher()?;
    //     self.watch_dirs.iter().for_each(|x|{
    //         watcher.watch(x.as_path(), RecursiveMode::NonRecursive);
    //     });
    //
    //     let mut params = IndexMap::new();
    //     while let Some(res) = rx.next().await {
    //         match res {
    //             Ok(event) => {
    //                 println!("changed: {:?}", &event);
    //                 if let Ok(new_headers) = Self::scan_db_headers(event.paths){
    //                     // dbg!(&new_headers);
    //                     for (path, new_header) in new_headers {
    //                         if let Some(old) = self.headers.get(&path) {
    //                             //未发生修改，直接跳过
    //                             if old.pdms_header.page_no == new_header.pdms_header.page_no { continue;  }
    //                             let range = (old.file_size..new_header.file_size);
    //                             params.insert(path.clone(), range);
    //                             self.headers.insert(path, new_header);
    //                         }
    //                     }
    //                 }
    //             }
    //             Err(e) => println!("watch error: {:?}", e),
    //         }
    //     }
    //
    //     Ok(())
    // }


    ///扫描出来每个db文件的 header信息
    pub fn scan_db_headers<P: AsRef<Path>>(
        paths: Vec<P>,
    ) -> anyhow::Result<IndexMap<PathBuf, DbPageBasicInfo>> {
        let mut result = IndexMap::new();
        for path in &paths {
            let mut io = PdmsIO::new(path, true);
            io.open()?;
            let basic_info = io.get_page_basic_info()?;
            // println!("basic info: {:#4X?}", &basic_info);
            let new_ses_no = basic_info.latest_ses_pageno + 1;
            result.insert(path.as_ref().to_path_buf(), basic_info);
        }

        Ok(result)
    }

    pub fn async_watcher() -> notify::Result<(RecommendedWatcher, Receiver<notify::Result<Event>>)> {
        let (mut tx, rx) = channel(1);

        // Automatically select the best implementation for your platform.
        // You can also access each implementation directly e.g. INotifyWatcher.
        let watcher = RecommendedWatcher::new(
            move |res| {
                futures::executor::block_on(async {
                    tx.send(res).await.unwrap();
                })
            },
            Config::default(),
        )?;

        Ok((watcher, rx))
    }
}

