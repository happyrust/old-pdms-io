use aios_core::{get_default_pdms_db_info, RefU64Vec, SUL_DB};
use aios_core::pdms_types::{PdmsElement, RefU64};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::convert::TryInto;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::mem::size_of;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use aios_core::NamedAttrValue::{IntegerType, LongType};
use aios_core::pdms_data::DataOperation;
use anyhow::anyhow;
use futures_util::{FutureExt, StreamExt};
use memchr::memmem::rfind_iter;
use crate::defines::*;
use parse_pdms_db::parse::*;

#[derive(Debug)]
pub struct PdmsIO {
    pub project: String,
    pub path: PathBuf,
    pub readonly: bool,
    pub file: Option<File>,
    pub ses_data_map: HashMap<u32, SessionPageData>,
}

impl PdmsIO {
    #[inline]
    pub fn read_bytes(&mut self, offset: u32, len: i32) -> anyhow::Result<Vec<u8>> {
        let file = self.get_file()?;
        let mut data = vec![];
        data.resize(len as usize, 0u8);
        file.seek(SeekFrom::Start(offset as u64))?;
        file.read_exact(&mut data)?;
        Ok(data)
    }
}

const REFNO_LEAF_INDEX_PAGE: [u8; 16] = [0x00u8, 0x00, 0x00, 0x05, 0x00, 0xCC, 0x47, 0xDF, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02];

impl PdmsIO {
    ///新建一个PdmsIO
    pub fn new<P: AsRef<Path>>(project: impl ToString, path: P, readonly: bool) -> Self {
        Self {
            project: project.to_string(),
            path: path.as_ref().to_path_buf(),
            readonly,
            file: None,
            ses_data_map: Default::default(),
        }
    }

    pub fn open(&mut self) -> anyhow::Result<()> {
        let file = File::options().read(self.readonly).open(&self.path)?;
        self.file = Some(file);
        Ok(())
    }

    fn get_file(&mut self) -> anyhow::Result<&mut File> {
        if self.file.is_none() {
            self.open()?;
        }
        Ok(self.file.as_mut().unwrap())
    }

    //todo 修改这里的每次都要读完的限制, read_to_end
    pub fn get_att_latest_pgno(&mut self) -> anyhow::Result<u32> {
        let mut file = self.get_file()?;
        let mut input = vec![];
        file.read_to_end(&mut input)?;
        let file_max_pgno = input.len() as u32 / 0x800;
        let mut pos_iter = rfind_iter(&input, &REFNO_LEAF_INDEX_PAGE[..]);
        let mut max_pgno = 0;
        while let Some(pos) = pos_iter.next() {
            // println!("Found leaf index page at: {:#04X?}", pgno);
            let index_data = self.read_index_data((pos / 0x800) as _)?;
            // dbg!(&index_data);
            max_pgno = index_data.refno_locs.iter().filter(|x|
            x.pgno <= file_max_pgno)
                .map(|x| x.pgno).max().unwrap_or_default().max(max_pgno);
            break;
        }
        Ok(max_pgno)
    }

    //todo 可以提前加载一些索引结构, 加载成BTree，可以很快的定位到page
    //todo 改成 search all ？是否需要根据参考号的某个属性来判断？
    pub fn search_refno_pgno(&mut self, refno: RefU64) -> anyhow::Result<RefnoDataLoc> {
        let basic_info = self.get_page_basic_info()?;
        let latest_index_pgno = basic_info.latest_ses_data.index_root_pageno;
        let mut index_data = self.read_index_data(latest_index_pgno)?;
        let mut level = index_data.level as i32;
        let (r0, r1) = (refno.get_0(), refno.get_1());
        while level >= 0 {
            let mut next_loc_index = if level == 0 {
                index_data.refno_locs.iter().position(|x| x.refno_0 == r0 && x.refno_1 == r1)
            } else {
                index_data.refno_locs.windows(2).position(
                    |x| (x[1].refno_0 > r0 && x[0].refno_0 <= r0)   //r0的范围找到后，可以停止
                        || (
                        (r0 >= x[0].refno_0 && r1 >= x[0].refno_1)
                            && (r0 <= x[1].refno_0 && r1 < x[1].refno_1)
                    )
                )
            };
            if level == 0 && next_loc_index.is_none() {
                break;
            }
            let indx = next_loc_index.unwrap_or(index_data.refno_locs.len() - 1);
            // dbg!(next_loc_index);
            let d = index_data.refno_locs[indx].clone();
            let next_pgno = d.pgno;
            // println!("index level {level}, next_pgno is {:#4X}", next_pgno);
            if level == 0 {
                // println!("index level {level}, found pgno is {:#4X}", next_pgno);
                return Ok(d);
            } else {
                index_data = self.read_index_data(next_pgno)?;
                level -= 1;
            }
        }

        Err(anyhow!("Can't find the att pos loc"))
    }

    ///获取单个element数据
    pub async fn get_element(&mut self, refno_offset: u64) -> anyhow::Result<EleData> {
        let mut file = self.get_file()?;
        let mut data = vec![0u8; 0x800];
        file.seek(SeekFrom::Start(refno_offset))?;
        file.read_exact(&mut data)?;

        let input = if data[..4] == [0, 0, 0, 0x7] {
            &data[4..]
        } else {
            &data[..]
        };
        parse_ele_data(input).await
    }

    //TODO 做一个不处理UDA的方法
    #[inline]
    pub async fn auto_get_element(&mut self, refno: RefU64) -> anyhow::Result<EleData> {
        let loc = self.search_refno_pgno(refno)?;
        self.get_element(loc.get_att_offset()).await
    }

    pub async fn auto_get_elements_deep(&mut self, refno: RefU64) -> anyhow::Result<HashMap<RefU64, EleData>> {
        let mut map = HashMap::new();
        let mut pendings = VecDeque::new();
        pendings.push_back(refno);
        while let Some(refno) = pendings.pop_front() {
            let ele = self.auto_get_element(refno).await?;
            pendings.extend(&*ele.children);
            map.insert(refno, ele);
        }
        Ok(map)
    }

    ///获得page的信息
    pub fn get_page_basic_info(&mut self) -> anyhow::Result<DbPageBasicInfo> {
        let pdms_header = self.read_pdms_header()?;
        // println!("{:#04X?}", &pdms_header);
        let latest_ses_pageno = pdms_header.latest_ses_pgno;
        let latest_ses_data = self.read_ses_data(latest_ses_pageno)?.clone();
        let file = self.get_file()?;
        Ok(DbPageBasicInfo {
            pdms_header,
            latest_ses_pageno,
            latest_ses_data,
            file_size: file.metadata().unwrap().len(),
        })
    }

    #[inline]
    pub fn read_pdms_header(&mut self) -> anyhow::Result<PdmsHeader> {
        let file = self.get_file()?;
        file.seek(SeekFrom::Start(0u64))?;
        let mut head_data = vec![];
        head_data.resize(size_of::<PdmsHeader>(), 0u8);
        file.read_exact(&mut head_data)?;
        let pdms_header = PdmsHeader::try_from(head_data.as_ref())?;
        Ok(pdms_header)
    }

    ///读取ses data
    #[inline]
    pub fn read_ses_data(&mut self, ses_pgno: u32) -> anyhow::Result<&SessionPageData> {
        if !self.ses_data_map.contains_key(&ses_pgno) {
            let file = self.get_file()?;
            let mut ses_data = vec![];
            ses_data.resize(size_of::<SessionPageData>(), 0u8);
            file.seek(SeekFrom::Start(ses_pgno as u64 * 0x800))?;
            file.read_exact(&mut ses_data)?;
            if let Ok(mut s) = SessionPageData::try_from(ses_data.as_ref()) {
                s.pgno = ses_pgno as _;
                self.ses_data_map.insert(ses_pgno, s);
            }
        }
        return self.ses_data_map.get(&ses_pgno).ok_or(anyhow!("Can't read ses page with {ses_pgno}."));
    }

    #[inline]
    pub fn read_index_data(&mut self, index_pgno: u32) -> anyhow::Result<IndexPageData> {
        let file = self.get_file()?;
        let mut ses_data = vec![];
        ses_data.resize(0x800, 0u8);
        file.seek(SeekFrom::Start(index_pgno as u64 * 0x800))?;
        file.read_exact(&mut ses_data)?;
        let ses_page_data = IndexPageData::try_from(ses_data.as_ref())?;
        Ok(ses_page_data)
    }

    ///指定 refno，收集它的历史数据
    pub fn collect_ele_history(&self, refno: RefU64) -> Vec<EleData> {
        let mut eles = vec![];
        //根据参考号的pgno，快速找到 sesno -> pgno 的映射
        //提前在 surreal 里存储？还是手动去搜索所有的 refno 数据

        eles
    }

    //todo add some date filter ? session filter
    pub async fn total_sync_sessions_to_db(&mut self) -> anyhow::Result<()> {
        use itertools::Itertools;

        let pdms_header = self.read_pdms_header().unwrap();
        let dbnum = pdms_header.db_num;
        let mut cur_ses_pgno = pdms_header.latest_ses_pgno;
        let project = self.project.clone();

        //显示出有哪些修改，使用 json diff 工具
        let mut refno_pgnos_map = BTreeMap::new();
        let mut step = 0;
        //遍历整个文件数据
        while cur_ses_pgno > 4 {
            // println!("cur_ses_pgno: {:#4X?}", cur_ses_pgno);
            //数据还是跟 pgno ?
            // let all_ents_in_ses = self.collect_refno_los_in_session(cur_ses_pgno as _).await;
            //历史数据是否需要存储的问题？
            // dbg!(&all_ents_in_ses);
            let all_locs = self.collect_refno_locs_in_session(cur_ses_pgno as _).await;
            dbg!(all_locs.len());

            let cur_ses_page = self.read_ses_data(cur_ses_pgno as _).unwrap();
            //先保存 session 数据到数据库
            let sql = format!("insert into e3d_ses {}",
                              cur_ses_page.gen_sur_json(project.as_str(), pdms_header.db_num));
            //执行 sql
            // SUL_DB.query(&sql).await.unwrap();

            let ses_id = cur_ses_page.get_id(project.as_str(), pdms_header.db_num);
            let mut all_relates = Vec::new();
            for (i, loc) in all_locs.iter().enumerate() {
                // let ele = self.get_element(loc.get_att_offset()).await.unwrap();
                // let sql = format!("insert into e3d_ele {}",
                //                   ele.gen_sur_json(project.as_str(), pdms_header.db_num, ses_id));
                // let relate_sql = format!("relate pe:{}->ses_pe_relate:['{}', {i}]->e3d_ses:{} set pgno={}, offset={};", ses_id, ses_id, loc.get_refno(), loc.pgno, loc.offset);

                all_relates.push(format!("{{ id:[e3d_ses:{}, {i}], in: pe:{}, out: e3d_ses:{}, pgno:{}, offset:{} }}",
                                         ses_id, loc.get_refno(), ses_id, loc.pgno, loc.offset));
                //如果 refno + pgno 对应的数据在数据库里已经存在，那就不插入
                //怎么知道这个数据是不是 old data?
                //读取所有历史数据就可以了, 在这里做个排序也可以
                // refno_pgnos_map.entry(loc.get_refno()).or_insert_with(BTreeSet::new).insert(loc.get_att_offset());
                refno_pgnos_map.entry(loc.get_refno()).or_insert_with(BTreeSet::new).insert((loc.pgno, cur_ses_page.sesno, loc.offset));
            }
            //修改、删除、增加，放在这里去加一个字段
            for chunk in all_relates.chunks(1000) {
                let relate_sql = format!("INSERT RELATION INTO ses_pe_relate [{}];", chunk.join(","));
                // println!("relates: {}", chunk.join(","));
                // SUL_DB.query(relate_sql).await.unwrap();
            }
            //直接通过数据库查是否最新？还是通过文件查找？
            //每个参考号都去拉取一遍，然后看看是不是最新的？

            // let offset = cur_ses_page.end_pgno * 0x800 + 0x4;
            // let bytes = io.read_bytes(offset, 4).unwrap();
            // let type_name = db1_dehash(u32::from_be_bytes(bytes.try_into().unwrap()));
            // dbg!(type_name);
            // println!("session pgno {:#4X}: {:#4X}", cur_ses_pgno, offset / 0x800);
            // dbg!((cur_ses_no, offset));
            // dbg!(cur_ses_page.last_ses_pageno);
            if step == 50{
                break;
            }
            if cur_ses_page.last_ses_pageno < 0 {
                break;
            }
            step += 1;
            cur_ses_pgno = cur_ses_page.last_ses_pageno as _;
            // dbg!(last_ses_no);
            // dbg!(cur_ses_page.get_timestamp());
            // dbg!(cur_ses_page.get_computer_name());
            // dbg!(cur_ses_page.get_comments_name());

            // break;
        }

        //refno_pgnos_map 查询里面 value 最多的项
        // let max_history_refno = refno_pgnos_map.iter().max_by_key(|x| x.1.len());
        // dbg!(&max_history_refno);

        //pe_history
        //pe_owner history 是否有必要
        //pe_owner 始终是最新的数据
        //pe_owner_history  为  pe_history 之间的关联关系？也有可能是 pe
        //如果 children 发生变化，确实需要记录这个，如果 pe 里没有的，那就是在pe_history 里面
        //NOUN_history
        //保存历史属性数据到数据库
        let mut type_att_map = BTreeMap::new();
        let mut found = false;
        //e3d_session 是否要绑定一个Operation log 的指向，还是直接可以对比两个session 就可以得到？
        //但是这样没法实现参考号查询自己是啥时候发生删除的，或者修改的
        for (refno, locs) in refno_pgnos_map.into_iter() {
            if locs.len() <= 1 {
                continue;
            }
            //表示有历史记录，后面存储的都是old data, 查询时需要和latest data 合着一起查询
            dbg!(locs.len());
            //（1）按着从小到大的顺序排列的，所以后面的是新的，可以判断构件是否被删除
            //如果是新增加的呢？怎么样维护这个是否新增的关系，这里就要比较这个 ses no 的关系了，在查询的时候，如果是按
            //历史记录查询，需要加个 sesno 的条件过滤，或者 pgno 的过滤，子节点的 pngo 不能超过某个pgno
            //删除了肯定是不能再加回去这个参考号的
            //是否需要弄个pe_history? 还是就放在 pe 里面？应该是都放在 pe 里，然后历史的数据需要加上，以为 pe 是唯一的
            //即使属性发生变化，也只是 pgno 的变化
            let mut prev_children = RefU64Vec::default();
            //todo 使用 chunk
            let mut ses_relates = Vec::new();
            // let mut owner_relates = Vec::new();
            let len = locs.len();
            let mut all_pes = Vec::new();
            //pe_owner 怎么处理？
            for (index, (pgno, sesno, offset)) in locs.into_iter().enumerate() {
                let addr = pgno as u64 * 0x800 + offset as u64 * 2;
                if let Ok(mut data) = self.get_element(addr).await{
                    let mut att = &mut data.whole_attmap.attmap;
                    // att.insert("PGNO".to_string(), LongType(offset as i64));
                    att.insert("PGNO".to_string(), IntegerType(pgno as _));
                    let mut pe = att.pe(dbnum);
                    //需要在这里检测是否和上一个比，有 delete 的变化，也就是比较 children
                    if !data.children.is_empty(){
                        // let mut found_deleted = false;
                        // for (&p, &q) in data.children.iter().zip(prev_children.iter()) {
                        //     if p != q {
                        //         // println!("{refno}: {} -> {}", q, p);
                        //         found = true;
                        //         found_deleted = true;
                        //         break;
                        //     }
                        // }
                        //TODO modified refnos
                        //过滤出删除的参考号
                        let all_deleted = prev_children.iter().cloned().filter(|x|{
                            !data.children.contains(x)
                        }).collect::<BTreeSet<_>>();

                        //过滤出新增的参考号
                        let all_added = data.children.iter().cloned().filter(|x|{
                            !prev_children.contains(x)
                        }).collect::<BTreeSet<_>>();


                        let ses_id = format!("{}_{}_{:0>6}", project, dbnum, sesno);
                        //插入删除的操作记录
                        //将删除的 pe 要重新插入回去，然后设置为 deleted
                        for (j, r) in all_deleted.iter().enumerate(){
                            let op: i32 = DataOperation::Deleted.into();
                            ses_relates.push(format!("{{ id:[e3d_ses:{}, {}], in: pe:{}, out: e3d_ses:{}, op: {} }}",
                                                     ses_id, len+j, r, ses_id, op));
                            //要读取到这个删除的 att
                            pe.deleted = true;
                            // all_deleted_pes.push(pe);
                        }

                        //插入新增的增加的记录
                        for r in &all_added{
                            let op: i32 = DataOperation::Added.into();
                            ses_relates.push(format!("{{ id:[e3d_ses:{}, {}], in: pe:{}, out: e3d_ses:{}, pgno:{}, offset:{}, op: {} }}",
                                                     ses_id, index, r, ses_id, pgno, offset, op));
                        }
                        //既不是新增，又不是删除，那就是修改
                        if !all_added.contains(&refno) && !all_deleted.contains(&refno) {
                            let op: i32 = DataOperation::Modified.into();
                            ses_relates.push(format!("{{ id:[e3d_ses:{}, {}], in: pe:{}, out: e3d_ses:{}, pgno:{}, offset:{}, op: {} }}",
                                                     ses_id, index, refno, ses_id, pgno, offset, op));
                        }

                        //修改的需要去比对属性数据

                        if !all_deleted.is_empty() || !all_added.is_empty() {
                            println!("{refno}_{pgno}: {:?}", prev_children);
                            println!("{refno}_{pgno}: {:?}", data.children);
                            println!("Deleted: {:?}", &all_deleted);
                            println!("Added: {:?}", &all_added);
                            //todo 需要将这个 relate 关系加回去，同时创建 pe，并设置为 delete
                            //todo 怎么查询这个构件是什么时候删除的呢，需要关联操作日志
                        }
                        prev_children = data.children.clone();
                    }
                    // if prev_children == data.children {
                    //     //新加的部分也要放在pe_relate里去
                    // }else{
                    //     prev_children = data.children.clone();
                    // }
                    all_pes.push(pe);
                    type_att_map.entry(att.get_type()).or_insert(Vec::new()).push(data);
                }else{
                    dbg!((pgno, offset));
                    break;
                }
            }

            if !ses_relates.is_empty(){
                let relate_sql = format!("INSERT RELATION INTO ses_pe_relate [{}];", ses_relates.join(","));
                // let relate_sql = format!("UPSERT RELATION INTO ses_pe_relate [{}];", all_relates.join(","));
                // println!("relates: {}", &relate_sql);
                SUL_DB.query(relate_sql).await.unwrap();
            }

            for chunk in all_pes.chunks(1000){
                let mut jsons = vec![];
                for pe in chunk{
                    jsons.push(pe.gen_sur_json(Some(pe.history_id())));
                }
                let sql = format!("INSERT IGNORE INTO pe_history [{}];", jsons.join(","));
                println!("sql is {}", &sql);
                SUL_DB.query(sql).await.unwrap();
            }

            if found{
                break;
            }
            // let sql = format!("insert into {}_history [{}]",ele.
            //                   jsons.join(","));
            // //执行 sql
            // SUL_DB.query(&sql).await.unwrap();
            // }
            // let max_pgno = kv.1.iter().max().unwrap();
            // let eles = self.collect_eles_in_session(*max_pgno).await;
            // println!("refno: {:#4X?}", refno);
            // println!("max_pgno: {:#4X?}", max_pgno);
            // println!("eles: {:#4X?}", eles.len());
            // println!("eles: {:#4X?}", eles);
        }
        //pe_owner_history 对pe 进行修正？还是直接存储这个children 关系？
        //先暂时不支持 relate 的历史纪录？还是反过来加入 relate 的 patch？

        return Ok(());
        //对 type_att_map 进行历史数据的保存
        //todo 后续可以改解析，都是用这个方法去保存数据
        for (type_name, ele_datas) in type_att_map.into_iter() {
            for es in ele_datas.chunks(1000) {
                let mut jsons = Vec::new();
                //pe 直接就加在 pe_relate，然后通过 pe_relate 去查看 pe 的 delete 属性
                //delete 属性后面要用起来
                for ele_data in es {
                    if let Some(json) = ele_data.att_map().gen_sur_json_with_id(ele_data.att_map().history_id()) {
                        jsons.push(json);
                    }
                }
                let sql = format!("insert into {}_history [{}]",
                                  type_name, jsons.join(","));
                SUL_DB.query(&sql).await.unwrap();
            }
        }


        Ok(())
    }

    //todo 对比两个 session，发生了哪些变化
    //old, new
    pub async fn compare_eles_between_sessions() {}

    pub async fn collect_refno_locs_in_session(&mut self, ses_pgno: u32) -> Vec<RefnoDataLoc> {
        //读取当前会话层有多少属性保存了，是否需要读取 index 数据，然后开始读取属性数据
        //过滤 index 里面的 pgno 大于当前会话的 pgno 的数据
        let (cur_end_pgno, last_ses_pageno, index_root_pageno) = {
            let d = self.read_ses_data(ses_pgno).unwrap();
            (d.end_pgno, d.last_ses_pageno, d.index_root_pageno)
        };
        //读取上一个ses_data
        let last_end_pgno = {
            let d = self.read_ses_data(last_ses_pageno as u32).unwrap();
            d.end_pgno
        };
        // dbg!((last_end_pgno, cur_end_pgno));
        //只要过滤所有 last_end_pgno 比这个大，比 cur_end_pgno 小的参考号即可
        //过滤 index page data 里面的数据
        let mut index_data = self.read_index_data(index_root_pageno).unwrap();
        // dbg!(index_data.level);
        let mut final_locs = vec![];
        let mut level = index_data.level as i32;
        self.filter_index_data(&index_data, &mut final_locs, last_end_pgno, cur_end_pgno, &mut level);

        final_locs
    }

    pub async fn collect_eles_in_session(&mut self, ses_pgno: u32) -> Vec<EleData> {
        let final_locs = self.collect_refno_locs_in_session(ses_pgno).await;
        let mut eles = vec![];

        // dbg!(&locs);
        // //根据这个RefnoDataLoc 读取到所有发生更新的 index 数据
        for loc in final_locs {
            let ele = self.get_element(loc.get_att_offset()).await.unwrap();
            eles.push(ele);
            // dbg!(&loc);
        }


        eles
    }

    //递归的写法去读取
    pub fn filter_index_data(&mut self, index_data: &IndexPageData, result_locs: &mut Vec<RefnoDataLoc>,
                             last_end_pgno: u32, cur_end_pgno: u32, level: &mut i32) -> Option<bool> {
        // let mut level = index_data?.level as i32;
        if index_data.refno_locs.is_empty() {
            return None;
        }
        let cur_locs = index_data.refno_locs.iter()
            .filter(|x| x.pgno > last_end_pgno && x.pgno < cur_end_pgno)
            .map(|x| x.clone())
            .collect::<Vec<_>>();
        if cur_locs.is_empty() {
            return None;
        }
        // dbg!(*level);
        if *level == 0 {
            // dbg!(&cur_locs[0]);
            result_locs.extend(cur_locs);
        } else {
            for l in cur_locs {
                // dbg!(l.pgno);
                if let Ok(next_index_data) = self.read_index_data(l.pgno) {
                    // if next_index_data.pfno != 253 {
                    //     continue;
                    // }
                    let mut next_level = next_index_data.level as i32;
                    //todo make clear why exist this situation
                    if next_level >= *level {
                        // dbg!((next_level, *level));
                        // dbg!((&l, next_index_data));
                    } else {
                        self.filter_index_data(&next_index_data, result_locs, last_end_pgno, cur_end_pgno, &mut next_level);
                    }
                }
            }
        }
        Some(true)
    }


    //直接读取中间这段数据的att index table，直接获取所有需要的数据
    pub async fn collect_increment_eles(
        &mut self,
        till_pageno: u32,
    ) -> anyhow::Result<HashMap<RefU64, EleData>> {
        let mut file = self.get_file()?;
        let mut input = vec![];
        let start = till_pageno as u64 * 0x800;
        #[cfg(feature = "debug_parse")]
        println!("Bytes start at : {:#04X?}", start);
        file.seek(SeekFrom::Start(start)).expect("collect_increment_eles");
        file.read_to_end(&mut input)?;
        let mut pos_iter = rfind_iter(&input, &REFNO_LEAF_INDEX_PAGE[..]);
        // let mut max_pgno = 0;
        let mut refno_data_offsets_map = BTreeMap::new();
        while let Some(mut pos) = pos_iter.next() {
            pos += start as usize;
            #[cfg(feature = "debug_parse")]
            println!("Found leaf index page at: {:#04X?}", pos);
            let index_data = self.read_index_data((pos / 0x800) as _)?;
            for x in index_data.refno_locs {
                if x.pgno < till_pageno {
                    continue;
                }
                let refno_att_offset = x.get_att_offset();
                #[cfg(feature = "debug_parse")]
                println!("Found loc: {:#04X?}, att_offset: {:#04X}", &x, refno_att_offset);
                let refno = RefU64::from_two_nums(x.refno_0, x.refno_1);
                if !refno_data_offsets_map.contains_key(&refno) {
                    refno_data_offsets_map.insert(
                        refno,
                        refno_att_offset,
                    );
                }
            }
        }

        let mut eles_map = HashMap::new();
        for (refno, offset) in refno_data_offsets_map {
            match self.get_element(offset).await {
                Ok(ele) => {
                    eles_map.insert(ele.refno, ele);
                }
                Err(e) => {
                    #[cfg(feature = "debug_parse")]
                    {
                        dbg!((refno, offset, e));
                    }
                }
            }
        }
        Ok(eles_map)
    }

    pub fn search_refno(&mut self, refno: RefU64) -> anyhow::Result<bool> {
        let file = self.get_file()?;
        file.seek(SeekFrom::Start(0u64))?;
        let mut head_data = vec![];
        head_data.resize(size_of::<PdmsHeader>(), 0u8);
        file.read_exact(&mut head_data)?;
        // dbg!(data[]);
        let pdms_header = PdmsHeader::try_from(head_data.as_ref()).unwrap();
        // println!("{:#04X?}", &pdms_header);

        let ses_addr = pdms_header.latest_ses_pgno * 0x800;
        // println!("Ses addr: {:#04X}", ses_addr);

        let mut ses_data = vec![];
        ses_data.resize(size_of::<SessionPageData>(), 0u8);
        file.seek(SeekFrom::Start(ses_addr as u64))?;
        file.read_exact(&mut ses_data)?;
        let ses_start_part = SessionPageData::try_from(ses_data.as_ref()).unwrap();
        // println!("{:#04X?}", &ses_start_part);

        //todo find this refno
        //when not found in current session page, should search back
        //=23584/5653
        //00 00 5C 20 00 00 16 15
        let indx_addr = ses_start_part.index_root_pageno * 0x800;
        // println!("index addr: {:#04X}", indx_addr);
        let mut data = vec![];
        data.resize(size_of::<RootIndexPage>(), 0u8);
        file.seek(SeekFrom::Start(indx_addr as u64))?;
        file.read_exact(&mut data)?;
        let index_root = RootIndexPage::try_from(data.as_ref()).unwrap();
        // println!("{:#04X?}", &index_root);

        let l = &index_root.lower_root;
        let u = &index_root.upper_root;

        // dbg!(l);
        // dbg!(u);
        // println!("{:#4X?} {:#4X?}", l.refno_0, l.refno_1);
        // println!("{:#4X?} {:#4X?}", u.refno_0, u.refno_1);

        //先搜索这个最大的范围，然后再缩小范围
        //here we need a loop to find the right location range
        let root_loc = if refno.get_0() == l.refno_0
            && (refno.get_1() >= l.refno_1 && refno.get_1() < u.refno_1)
        {
            Some(l)
        } else {
            None
        };

        let root_loc = root_loc.unwrap();

        ///一步步找到目标refno
        let target_root_addr = root_loc.page_no * 0x800;
        println!("cur index addr: {:#04X}", target_root_addr);
        let mut data = vec![0u8; 0x800];
        file.seek(SeekFrom::Start(target_root_addr as u64))?;
        file.read_exact(&mut data)?;
        let index_pgid = RefnoIndexPage::try_from(data.as_ref()).unwrap();
        // println!("{:#04X?}", &index_pgid);

        //todo 使用memchr直接去找到目标refno
        let target_refno_loc = index_pgid
            .data_locs
            .iter()
            .position(|x| x.refno_0 == refno.get_0() && x.refno_1 > refno.get_1());
        let target_refno_pgid = if let Some(mut t) = target_refno_loc {
            if t > 0 {
                t = t - 1;
            }
            let target_refno_pgid = &index_pgid.data_locs[t];
            println!("target_refno_pgid: {:#04X?}", target_refno_pgid);
            Some(target_refno_pgid)
        } else {
            None
        };

        //todo temp
        let target_refno_pgid = target_refno_pgid.unwrap();

        let target_loc = target_refno_pgid.page_no * 0x800;
        println!("cur index addr: {:#04X}", target_loc);
        let mut data = vec![0u8; 0x800];
        file.seek(SeekFrom::Start(target_loc as u64))?;
        file.read_exact(&mut data)?;
        let index_data = IndexPageData::try_from(data.as_ref()).unwrap();
        // println!("{:#04X?}", &index_data);

        let target_data_loc = index_data
            .refno_locs
            .iter()
            .find(|x| x.refno_0 == refno.get_0() && x.refno_1 == refno.get_1());
        // println!("{:#04X?}", &target_data_loc);

        if let Some(l) = target_data_loc {
            let _loc = l.pgno * 0x800 + l.offset as u32 * 2;
            // println!("data loc {:#04X?}", loc);
            // let element = self.get_element( loc)?;
            // dbg!(&element);
        }

        let _upper_root_addr = u.page_no * 0x800;
        // println!("last index addr: {:#04X}", upper_root_addr);

        Ok(true)
    }
}
