use crate::defines::*;
use aios_core::pdms_data::DataOperation;
use aios_core::pdms_types::{EleOperation, PdmsElement, RefU64};
use aios_core::{
    get_default_pdms_db_info, query_refno_sesno, NamedAttrMap, NamedAttrValue, RefU64Vec,
    RefnoEnum, RefnoSesno, SUL_DB,
};
use anyhow::anyhow;
use dashmap::DashMap;
use futures_util::{FutureExt, StreamExt};
use memchr::memmem::rfind_iter;
use parse_pdms_db::parse::*;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::convert::TryInto;
use std::fmt::format;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::mem::size_of;
use std::ops::{Range, RangeInclusive};
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Debug)]
pub struct PdmsIO {
    pub project: String,
    pub path: PathBuf,
    pub readonly: bool,
    pub dbnum: i32,
    pub file: Option<File>,
    //sesno
    pub ses_data_map: HashMap<u32, SessionPageData>,
    pub sesno_pgno_map: BTreeMap<i32, u32>,
    //start pgno 和 end pgno
    pub ses_range_map: BTreeMap<i32, Range<u32>>,
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

const REFNO_LEAF_INDEX_PAGE: [u8; 16] = [
    0x00u8, 0x00, 0x00, 0x05, 0x00, 0xCC, 0x47, 0xDF, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x02,
];

#[derive(Debug)]
enum SesSqlType {
    SesJson(Vec<String>),
    PeSesSql(Vec<String>),
    PeHJson(Vec<String>),
    PeOwnerSql(Vec<String>),
}

impl PdmsIO {
    ///新建一个PdmsIO
    pub fn new<P: AsRef<Path>>(project: impl ToString, path: P, readonly: bool) -> Self {
        Self {
            project: project.to_string(),
            path: path.as_ref().to_path_buf(),
            readonly,
            file: None,
            dbnum: 0,
            ses_data_map: Default::default(),
            sesno_pgno_map: Default::default(),
            ses_range_map: Default::default(),
        }
    }

    pub fn open(&mut self) -> anyhow::Result<()> {
        let file = File::options().read(self.readonly).open(&self.path)?;
        self.file = Some(file);
        self.init_ses_range_map()?;
        Ok(())
    }

    fn get_file(&mut self) -> anyhow::Result<&mut File> {
        if self.file.is_none() {
            self.open()?;
        }
        Ok(self.file.as_mut().unwrap())
    }

    ///收集文件中的所有 ses 范围
    pub fn init_ses_range_map(&mut self) -> anyhow::Result<()> {
        let pdms_header = self.read_pdms_header()?;
        let mut cur_ses_pgno = pdms_header.latest_ses_pgno;
        let mut map = BTreeMap::new();
        let mut sesno_pgno_map = BTreeMap::new();
        self.dbnum = pdms_header.db_num as _;
        //遍历整个文件数据, 从最新的最前的遍历
        while cur_ses_pgno > 4 {
            let cur_ses_page = self.read_ses_data(cur_ses_pgno as _)?;
            let range = (cur_ses_page.last_ses_pageno as u32)..cur_ses_pgno;
            map.insert(cur_ses_page.sesno, range);
            sesno_pgno_map.insert(cur_ses_page.sesno, cur_ses_pgno);
            if cur_ses_page.last_ses_pageno < 0 {
                break;
            }
            cur_ses_pgno = cur_ses_page.last_ses_pageno as _;
        }

        self.ses_range_map = map;
        self.sesno_pgno_map = sesno_pgno_map;

        Ok(())
    }

    /// 根据pgno, 获取 sesno
    pub fn get_sesno(&self, pgno: u32) -> Option<u32> {
        for (sesno, range) in &self.ses_range_map {
            if range.contains(&pgno) {
                return Some(*sesno as _);
            }
        }
        None
    }

    pub fn read_latest_ses_page(&mut self) {}

    ///获取最新属性pgno
    pub fn get_latest_att_pgno(&mut self) -> anyhow::Result<u32> {
        let header = self.read_pdms_header()?;
        let ses_pgno = header.latest_ses_pgno;
        // let ses_data = self.read_ses_data(ses_pgno)?;
        let all_locs = self.collect_refno_locs_in_session(ses_pgno as _);
        let max_pgno = all_locs.iter().map(|x| x.pgno).max().unwrap_or_default();
        Ok(max_pgno)
    }

    ///获得最新的饿sesno
    pub fn get_latest_att_sesno(&mut self) -> anyhow::Result<u32> {
        let header = self.read_pdms_header()?;
        let latest_ses_data = self.read_ses_data(header.latest_ses_pgno)?;
        Ok(latest_ses_data.sesno as _)
    }

    pub fn get_att_latest_pgno_old(&mut self) -> anyhow::Result<u32> {
        let mut file = self.get_file()?;
        let mut input = vec![];
        file.read_to_end(&mut input)?;
        let file_max_pgno = input.len() as u32 / 0x800;
        let mut pos_iter = rfind_iter(&input, &REFNO_LEAF_INDEX_PAGE[..]);
        let mut max_pgno = 0;
        while let Some(pos) = pos_iter.next() {
            let pgno = (pos / 0x800) as _;
            println!("Found leaf index page at: {:#04X?}", pgno);
            let index_data = self.read_index_data(pgno)?;
            dbg!(&index_data);
            max_pgno = index_data
                .refno_locs
                .iter()
                .filter(|x| x.pgno <= file_max_pgno)
                .map(|x| x.pgno)
                .max()
                .unwrap_or_default()
                .max(max_pgno);
            break;
        }
        Ok(max_pgno)
    }

    pub fn search_refno_pgno(&mut self, refno: RefU64) -> anyhow::Result<RefnoDataLoc> {
        let basic_info = self.get_page_basic_info()?;
        let latest_index_pgno = basic_info.latest_ses_data.index_root_pageno;
        let mut index_data = self.read_index_data(latest_index_pgno)?;
        let mut level = index_data.level as i32;
        let (r0, r1) = (refno.get_0(), refno.get_1());
        while level >= 0 {
            let mut next_loc_index = if level == 0 {
                index_data
                    .refno_locs
                    .iter()
                    .position(|x| x.refno_0 == r0 && x.refno_1 == r1)
            } else {
                index_data.refno_locs.windows(2).position(|x| {
                    (x[1].refno_0 > r0 && x[0].refno_0 <= r0)   //r0的范围找到后，可以停止
                        || (
                        (r0 >= x[0].refno_0 && r1 >= x[0].refno_1)
                            && (r0 <= x[1].refno_0 && r1 < x[1].refno_1)
                    )
                })
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
        let mut ele_data = parse_ele_data(input, (refno_offset / 0x800) as _).await?;
        let pgno = (refno_offset / 0x800) as u32;
        let sesno = self.get_sesno(pgno).unwrap_or_default() as i32;
        ele_data.att_map_mut().set_sesno(sesno);
        Ok(ele_data)
    }

    //TODO 做一个不处理UDA的方法
    #[inline]
    pub async fn auto_get_element(&mut self, refno: RefU64) -> anyhow::Result<EleData> {
        let loc = self.search_refno_pgno(refno)?;
        let mut ele_data = self.get_element(loc.get_att_offset()).await?;
        Ok(ele_data)
    }

    pub async fn auto_get_elements_deep(
        &mut self,
        refno: RefU64,
    ) -> anyhow::Result<HashMap<RefU64, EleData>> {
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
            // ses_data.resize(size_of::<SessionPageData>(), 0u8);
            ses_data.resize(PAGE_SIZE, 0u8);
            let offset = ses_pgno as u64 * PAGE_SIZE as u64;
            file.seek(SeekFrom::Start(offset))?;
            file.read_exact(&mut ses_data)?;
            // dbg!(ses_pgno);
            SessionPageData::try_from(ses_data.as_ref()).unwrap();
            if let Ok(mut s) = SessionPageData::try_from(ses_data.as_ref()) {
                s.pgno = ses_pgno as _;
                // dbg!(ses_pgno);
                self.ses_data_map.insert(ses_pgno, s);
            }
        }
        return self
            .ses_data_map
            .get(&ses_pgno)
            .ok_or(anyhow!("Can't read ses page with {ses_pgno}."));
    }

    #[inline]
    pub fn read_index_data(&mut self, index_pgno: u32) -> anyhow::Result<IndexPageData> {
        let file = self.get_file()?;
        let mut index_data = vec![];
        index_data.resize(PAGE_SIZE, 0u8);
        file.seek(SeekFrom::Start(index_pgno as u64 * PAGE_SIZE as u64))?;
        file.read_exact(&mut index_data)?;
        let index_page_data = IndexPageData::try_from(index_data.as_ref())?;
        Ok(index_page_data)
    }

    ///指定 refno，收集它的历史数据
    pub fn collect_ele_history(&self, refno: RefU64) -> Vec<EleData> {
        let mut eles = vec![];
        //根据参考号的pgno，快速找到 sesno -> pgno 的映射
        //提前在 surreal 里存储？还是手动去搜索所有的 refno 数据

        eles
    }

    ///存储所有的参考号和对应的 sesno 数据
    /// 返回一个历史参考号集合，值为所有的位置
    pub async fn store_all_refno_sesno_map(
        &mut self,
    ) -> anyhow::Result<BTreeMap<RefU64, BTreeSet<(u64, u32)>>> {
        let mut history_loc_map: BTreeMap<RefU64, BTreeSet<(u64, u32)>> = BTreeMap::new();
        let pdms_header = self.read_pdms_header().unwrap();
        let dbnum = pdms_header.db_num;
        let mut cur_ses_pgno = pdms_header.latest_ses_pgno;
        //收集所有的 session 和 refno 的对应关系
        //表pe_ses_h:  id([refno, sesno]), refno(指向最新？), sesno, offset, dbnum, ses_table
        let mut pe_ses_sqls = Vec::new();
        // SUL_DB.query("remove table ses;").await.unwrap();
        //使用 channel 来保存 sql 数据
        let (tx, rx) = flume::unbounded::<SesSqlType>();
        let mut handles = Vec::new();
        //开启一个保存 pe_ses_h 的线程
        let handle = tokio::spawn(async move {
            while let Ok(values) = rx.try_recv() {
                match values {
                    //保存 session 数据
                    SesSqlType::SesJson(values) => {
                        for chunk in values.chunks(100) {
                            //插入 json 数据
                            let sql = format!("INSERT IGNORE INTO  ses [{}];", chunk.join(","));
                            // println!("ses sql: {}", &sql);
                            SUL_DB.query(sql).await.unwrap();
                        }
                    }
                    SesSqlType::PeSesSql(values) => {
                        for chunk in values.chunks(100) {
                            let sql = format!("INSERT IGNORE INTO  pe_ses_h (id, refno, sesno, offset, dbnum, ses) VALUES {};", chunk.join(","));
                            SUL_DB.query(sql).await.unwrap();
                        }
                    }
                    _ => {}
                }
            }
        });
        handles.push(handle);
        // 跳过没有变化的数据，需要用个hash 来记录
        let mut latest_refno_map = DashMap::new();
        while cur_ses_pgno > 4 {
            // if cur_ses_pgno == 716 {
            //     dbg!(cur_ses_pgno);
            // }
            let all_locs = self.collect_refno_locs_in_session(cur_ses_pgno as _);
            let cur_ses_page = self.read_ses_data(cur_ses_pgno as _).unwrap().clone();
            let sesno = cur_ses_page.sesno;
            // dbg!(sesno);
            // if sesno < 730 {
            //     break;
            // }
            let ses_id = cur_ses_page.get_id(dbnum);
            tx.send(SesSqlType::SesJson(vec![cur_ses_page.gen_sur_json(dbnum)]));

            for loc in all_locs {
                let refno = loc.get_refno();
                let offset = loc.get_att_offset();
                let Some(sesno) = self.get_sesno( (offset / 0x800) as _ ) else{
                    continue;
                };
                //需要记录所有的 offset 数据，如果有两个以上的，代表有历史数据，需要在后面做比较
                //根据读取的数据判断是否有增删改
                history_loc_map.entry(refno).or_default().insert((offset, sesno));
                let is_latest = !latest_refno_map.contains_key(&refno);
                //如果是最新的，就不需要保存到历史数据
                if is_latest {
                    latest_refno_map.insert(refno, loc);
                    continue;
                }
                let id = format!("['{}', {}]", refno, sesno);
                pe_ses_sqls.push(format!(
                    "({}, {}, {}, {}, {}, ses:[{}, {}])",
                    id,
                    refno.to_pe_key(),
                    sesno,
                    offset,
                    dbnum,
                    ses_id[0],
                    ses_id[1]
                ));
            }
            //按 chunks 保存数据
            if pe_ses_sqls.len() > 100 {
                tx.send(SesSqlType::PeSesSql(std::mem::take(&mut pe_ses_sqls)))
                    .unwrap();
            }

            if cur_ses_page.last_ses_pageno < 0 {
                break;
            }
            cur_ses_pgno = cur_ses_page.last_ses_pageno as _;
        }
        if pe_ses_sqls.len() > 0 {
            tx.send(SesSqlType::PeSesSql(pe_ses_sqls)).unwrap();
        }
        //关闭 channel
        drop(tx);
        for handle in handles {
            handle.await.unwrap();
        }

        //去掉 value 的长度为 1 的数据
        // history_loc_map.retain(|_, v| v.len() > 1);
        Ok(history_loc_map)
    }

    //todo 可以指定 sesno 的范围去更新历史数据
    pub async fn sync_history(&mut self) -> anyhow::Result<()> {
        let history_pe_map = self.store_all_refno_sesno_map().await?;
        dbg!(&history_pe_map.len());
        // 遍历所有的 offset, 读取属性数据，得到 attmap
        // let mut ses_map = HashMap::new();
        let dbnum = self.dbnum;
        let mut pe_owner_h_relates = Vec::new();
        let mut all_his_pe_json = Vec::new();
        let mut all_his_json = Vec::new();

        let mut all_his_att_json_map: HashMap<String, Vec<String>> = HashMap::new();
        let mut pe_op_map: HashMap<RefnoEnum, (EleOperation, u32)> = HashMap::new();
        let mut ses_op_map: HashMap<u32, Vec<EleOperation>> = HashMap::new();
        let debug_refno: RefU64 = "17496/171606".into();
        //将历史纪录都存储在 his_relate 里， owner 为当前最新的 pe
        //如果没有历史记录，则不存储，减小额外的存储
        let mut deleted_refnos_map = BTreeMap::new();
        //只添加了一次的数据纪录,  todo 需要排查是否有数据在删除里，需要特殊处理
        let mut added_only_refnos_map = BTreeMap::new();
        for (&refno, offset_set) in &history_pe_map {
            let is_debug = debug_refno == refno;
            let mut prev_children = Vec::new();
            let mut prev_att_json = None;
            let loc_len = offset_set.len();
            //如果后面有删除的动作，则需要再加一个重新插入删除的数据，应该还原到pe:[] 的历史 id 中，啥时候
            // 被删除的，需要把历史数据还原回来，数据就是一个简单的标记位就行？
            // 如果数据只有一个的时候，会以为 pe 里有数据，其实没有
            let mut is_single_his = loc_len == 1;
            let mut prev_sesno = 0;
            let mut all_sesnos = BTreeSet::new();
            for (i, &(offset, sesno)) in offset_set.iter().enumerate() {
                // 只有一个版本的情况，直接添加，都是最新的
                // if is_debug {
                //     dbg!(offset);
                //     dbg!(sesno);
                //     dbg!(&offset_set);
                // } else {
                //     continue;
                // }
                //也有可能是删除了的情况
                if loc_len == 1 {
                    ses_op_map.entry(sesno).or_default().push(EleOperation::Add);
                    added_only_refnos_map.insert(refno, offset);
                    break;
                }
                let is_last = i == loc_len - 1;
                let Ok(ele_data) = self.get_element(offset).await else {
                    continue;
                };
                let att = ele_data.att_map();
                let mut pe = att.pe(dbnum);
                if !is_last {
                    all_sesnos.insert(sesno);
                }
                if is_debug {
                    dbg!(&att);
                }

                //如果是第二个 json 开始，都是 modified
                //todo需要实际检查是否真的 json 数据发生变化
                if prev_att_json.is_some() {
                    let refno_sesno = RefnoSesno::new(refno, sesno);
                    if is_last {
                        pe_op_map.insert(refno.into(), (EleOperation::Modified, prev_sesno));
                    } else {
                        pe_op_map.insert(refno_sesno.into(), (EleOperation::Modified, prev_sesno));
                    }
                    ses_op_map
                        .entry(sesno)
                        .or_default()
                        .push(EleOperation::Modified);
                } else {
                    ses_op_map.entry(sesno).or_default().push(EleOperation::Add);
                }

                //和 prev_children 对比，如果在 prev 不在 current，则为删除，在 current，不在 prev，则为新增
                for &child in ele_data.children.iter() {
                    let refno_sesno = RefnoSesno::new(child, sesno);
                    //如果是 add，在当前 sesno 一定会有
                    if !prev_children.contains(&child) {
                        //默认其实就是 add
                        pe_op_map.insert(refno_sesno.into(), (EleOperation::Add, prev_sesno));
                        ses_op_map.entry(sesno).or_default().push(EleOperation::Add);
                    }
                }
                if is_debug {
                    // dbg!(&prev_children);
                    // dbg!(&ele_data.children);
                }
                for &child in prev_children.iter() {
                    let refno_sesno = RefnoSesno::new(child, sesno);
                    //如果是 add，在当前 sesno 一定会有
                    if !ele_data.children.contains(&child) {
                        // dbg!(&ele_data.children);
                        // dbg!(&prev_children);
                        deleted_refnos_map.insert(child, sesno);
                        //todo 需要在后面更新回来找到正确的结果？
                        let (_, latest_sesno) = history_pe_map
                            .get(&child)
                            .as_ref()
                            .unwrap()
                            .iter()
                            .rev()
                            .next()
                            .cloned()
                            .unwrap_or_default();
                        //todo 在 map 里可以提前存储
                        // dbg!(latest_sesno);
                        pe_op_map.insert(
                            refno_sesno.into(),
                            (EleOperation::Deleted, latest_sesno as _),
                        );
                        ses_op_map
                            .entry(sesno)
                            .or_default()
                            .push(EleOperation::Deleted);
                    }
                }
                //如果是最后一个参考号的位置，直接退出，不用去保存到历史数据，因为是最新的数据
                if is_last {
                    break;
                }
                //需要获得这个属性里所有是参考号的对应的 sesno
                let mut refno_sesno_map = att.build_refno_sesno_map(sesno, dbnum).await?;
                let owner_sesno = refno_sesno_map.get(&pe.owner.refno()).cloned().unwrap_or(0);
                let pe_json = pe.gen_sur_json_with_sesno(sesno as _, owner_sesno as _);
                all_his_pe_json.push(pe_json);
                let Some(att_json) = att.gen_sur_json_with_sesno(sesno as _, &refno_sesno_map) else {
                    continue;
                };
                prev_att_json = Some(att_json.clone());
                //保存his_relate 数据
                let ses_refno = RefnoSesno::new(refno, sesno);

                //todo 如果没有真的发生变化，其实可以不保存这个数据
                all_his_att_json_map
                    .entry(att.get_type())
                    .or_default()
                    .push(att_json);
                if all_his_pe_json.len() > 100 {
                    println!("all_his_pe_json: {}", &all_his_pe_json.len());
                    //直接执行 sql
                    let sql = format!("INSERT IGNORE INTO pe [{}];", all_his_pe_json.join(","));
                    SUL_DB.query(sql).await.unwrap();
                    all_his_pe_json.clear();
                }
                //保存 pe_owner history 的 relate 关系, owner 的 relate 关系 pe->owner
                let children = &ele_data.children;
                let owner_relates = Self::gen_owner_relates_h(&history_pe_map, &children.0, pe.refno.refno(), sesno, dbnum)
                    .await?;
                // dbg!(&owner_relates);
                pe_owner_h_relates.extend(owner_relates);

                prev_children = children.to_vec();
                prev_sesno = sesno;
            }

            //只存储历史的 refno纪录
            if !all_sesnos.is_empty() {
                let his_pe_keys = all_sesnos
                    .iter()
                    .map(|sesno| format!("pe:['{}', {}]", refno, sesno))
                    .collect::<Vec<_>>()
                    .join(",");
                all_his_json.push(format!(
                    r#"{{ id: his_pe:{0}, refnos: [{1}] }}"#,
                    refno.to_string(),
                    &his_pe_keys
                ));
            }

            if all_his_json.len() > 100 {
                let sql = format!("INSERT IGNORE INTO  his_pe [{}];", all_his_json.join(","));
                SUL_DB.query(sql).await.unwrap();
                all_his_json.clear();
            }
            if pe_owner_h_relates.len() > 100 {
                println!("pe_owner_h_relates: {}", &pe_owner_h_relates.len());
                //直接执行 sql
                let sql = format!(
                    "INSERT RELATION INTO pe_owner [{}];",
                    pe_owner_h_relates.join(",")
                );
                // if is_debug{
                //     println!("sql is : {}", &sql);
                // }
                SUL_DB.query(sql).await.unwrap();
                pe_owner_h_relates.clear();
            }
        }
        //检查 deleted_refnos 是否有在 add_only_refnos 中，如果有，则删除
        dbg!(&deleted_refnos_map);
        dbg!(&added_only_refnos_map.len());
        let mut no_modify_delete_refnos_map = BTreeMap::new();
        if !deleted_refnos_map.is_empty() && !added_only_refnos_map.is_empty() {
            for (&refno, &sesno) in &deleted_refnos_map {
                if added_only_refnos_map.contains_key(&refno) {
                    let offset = added_only_refnos_map.remove(&refno).unwrap();
                    no_modify_delete_refnos_map.insert(refno, (sesno, offset));
                }
            }

            if !no_modify_delete_refnos_map.is_empty() {
                // dbg!(&need_delete_refnos);
                //只出现过一次，然后被判断为删除的，需要还原为原来的数据
                for (refno, (del_sesno, offset)) in no_modify_delete_refnos_map {
                    // SUL_DB.query(sql).await.unwrap();
                    // let sql = format!("UPSERT pe:['{}', {}]", refno.to_pe_key(), sesno);
                    // SUL_DB.query(sql).await.unwrap();
                    let Some(add_sesno) = self.get_sesno((offset / 0x800) as _) else {
                        continue;
                    };
                    // ses_op_map.entry(add_sesno).or_default().pop();
                    let Ok(ele_data) = self.get_element(offset).await else {
                        continue;
                    };
                    let att = ele_data.att_map();
                    let mut pe = att.pe(dbnum);

                    let mut refno_sesno_map = att.build_refno_sesno_map(add_sesno, dbnum).await?;
                    let owner_sesno = refno_sesno_map.get(&pe.owner.refno()).cloned().unwrap_or(0);
                    let pe_json = pe.gen_sur_json_with_sesno(add_sesno as _, owner_sesno as _);
                    all_his_pe_json.push(pe_json);
                    let Some(att_json) =
                        att.gen_sur_json_with_sesno(add_sesno as _, &refno_sesno_map)
                    else {
                        continue;
                    };
                    all_his_att_json_map
                        .entry(att.get_type())
                        .or_default()
                        .push(att_json);
                    all_his_json.push(format!(
                        r#"{{ id: his_pe:{0}, refnos: [pe:['{0}', {del_sesno}], pe:{0}] }}"#,
                        refno.to_string(),
                    ));

                    let children = &ele_data.children;
                    let owner_relates = Self::gen_owner_relates_h(&history_pe_map, &children.0, pe.refno.refno(), add_sesno, dbnum)
                        .await?;
                    pe_owner_h_relates.extend(owner_relates);
                }
            }
        }

        if pe_owner_h_relates.len() > 0 {
            println!("pe_owner_h_relates: {}", &pe_owner_h_relates.len());
            //直接执行 sql
            let sql = format!(
                "INSERT RELATION INTO pe_owner [{}];",
                pe_owner_h_relates.join(",")
            );
            // println!("relation sql is {}", sql);
            SUL_DB.query(sql).await.unwrap();
        }
        if all_his_pe_json.len() > 0 {
            // println!("all_his_pe_json: {}", &all_his_pe_json.len());
            //直接执行 sql
            let sql = format!("INSERT IGNORE INTO  pe [{}];", all_his_pe_json.join(","));
            SUL_DB.query(sql).await.unwrap();
        }
        if all_his_json.len() > 0 {
            let sql = format!("INSERT IGNORE INTO  his_pe [{}];", all_his_json.join(","));
            // println!("sql: {}", &sql);
            SUL_DB.query(sql).await.unwrap();
        }
        //保存历史属性数据
        for (att_type, att_jsons) in all_his_att_json_map {
            //使用 chunk
            for chunk in att_jsons.chunks(100) {
                let sql = format!("INSERT IGNORE INTO  {}_H [{}];", att_type, chunk.join(","));
                SUL_DB.query(sql).await.unwrap();
            }
        }

        //执行 pe_op_map, 更新 pe 数据
        //update 这三个数量到 ses 表
        let dbnum = self.dbnum;
        for (sesno, ops) in ses_op_map {
            let add_cnt = ops.iter().filter(|op| **op == EleOperation::Add).count();
            let mod_cnt = ops
                .iter()
                .filter(|op| **op == EleOperation::Modified)
                .count();
            let del_cnt = ops
                .iter()
                .filter(|op| **op == EleOperation::Deleted)
                .count();
            let sql = format!(
                "UPDATE ses:[{dbnum}, {sesno}] set add_cnt={}, mod_cnt={}, del_cnt={};",
                add_cnt, mod_cnt, del_cnt
            );
            SUL_DB.query(sql).await.unwrap();
        }
        //update 修改状态到pe 表
        for (refno_sesno, (op, prev_sesno)) in pe_op_map {
            if op == EleOperation::Add {
                continue;
            }
            // let id = if no_modify_delete_refnos_map.contains_key(&refno_sesno.refno()) {
            let id = if op == EleOperation::Deleted {
                //删除需要都更新到pe
                refno_sesno.refno().to_pe_key()
            } else {
                refno_sesno.to_pe_key()
            };
            let mut sql = if refno_sesno.sesno().unwrap_or_default() == 0 {
                format!("UPSERT {} set op={}", id, op.into_num(),)
            } else {
                format!(
                    "UPSERT {} set op={}, sesno={}",
                    id,
                    op.into_num(),
                    refno_sesno.sesno().unwrap(),
                )
            };

            if prev_sesno != 0 {
                sql.push_str(&format!(
                    ", old_pe=pe:['{}', {prev_sesno}]",
                    refno_sesno.refno().to_string()
                ));
            }

            if op == EleOperation::Deleted {
                sql.push_str(&format!(", dbnum={dbnum}"));
            }

            SUL_DB.query(sql).await.unwrap();
        }
        Ok(())
    }

     /// 生成 owner 的 relate 关系，只生成历史数据
     pub async fn gen_owner_relates_h(his_map: &BTreeMap<RefU64, BTreeSet<(u64, u32)>>, children: &[RefU64], owner: RefU64, sesno: u32, dbnum: i32) -> anyhow::Result<Vec<String>> {
        let mut pe_owner_h_relates = Vec::new();
        for (index, &child) in children.iter().enumerate() {
            let (mut child_sesno, latest_sesno) = query_refno_sesno(child, sesno, dbnum).await?;
            //如果 child 没有历史数据，而且在最新的 pe 里没有这个数据
            if latest_sesno == 0 && child_sesno == 0 {
                let Some(locs) = his_map.get(&child) else {
                    continue;
                };
                //不超过当前 sesno 的的最大 sesno
                child_sesno = locs.iter().rev().find(|x| x.1 <= sesno).map(|x| x.1).unwrap_or(0);
                // dbg!((child, child_sesno));
            }
            //child id 需要去 pe_ses 里查询得到最近的那个版本
            //如果是历史数据，加上 old 的标签
            if child_sesno != 0 {
                pe_owner_h_relates.push(
                    format!(r#"{{ id: pe_owner:['{0}_{sesno}', {index}], in: pe:['{1}',{child_sesno}],
                        out: pe:['{0}', {sesno}],  old: true }}"#,
                            owner, child)
                );
            } else {
                // dbg!((child, child_sesno, refno, sesno));
                pe_owner_h_relates.push(
                    format!(r#"{{ id: pe_owner:['{0}_{sesno}', {index}], in: pe:{1}, out: pe:['{0}', {sesno}], old: true }}"#,
                            owner, child)
                );
            }
        }
        Ok(pe_owner_h_relates)
    }


    /// 同步所有 session 数据到数据库
    //todo add some date filter ? session filter
    pub async fn total_sync_sessions_to_db(&mut self) -> anyhow::Result<()> {
        // use itertools::Itertools;

        // //删除所有的历史数据
        // SUL_DB.query("DELETE  e3d_ses;").await.unwrap();
        // SUL_DB.query("DELETE  pe_h;").await.unwrap();
        // SUL_DB.query("DELETE  ses_pe_relate;").await.unwrap();

        // let pdms_header = self.read_pdms_header().unwrap();
        // let dbnum = pdms_header.db_num;
        // let mut cur_ses_pgno = pdms_header.latest_ses_pgno;
        // let project = self.project.clone();

        // //显示出有哪些修改，使用 json diff 工具
        // let mut step = 0;
        // //遍历整个文件数据, 从最新的最前的遍历
        // let mut latest_refno_map = DashMap::new();
        // let mut all_children_map: DashMap<RefU64, RefU64Vec> = DashMap::new();
        // let mut all_relates = Vec::new();
        // //pe_owner_h 的添加
        // while cur_ses_pgno > 4 {
        //     //数据还是跟 pgno ?
        //     // let all_ents_in_ses = self.collect_refno_los_in_session(cur_ses_pgno as _).await;
        //     //历史数据是否需要存储的问题？
        //     // dbg!(&all_ents_in_ses);
        //     let all_locs = self.collect_refno_locs_in_session(cur_ses_pgno as _);
        //     // dbg!(all_locs.len());

        //     let cur_ses_page = self.read_ses_data(cur_ses_pgno as _).unwrap().clone();
        //     //保存session 数据
        //     // Self::save_ses_data(&pdms_header, &project, &cur_ses_page).await;

        //     let mut all_his_att_sql = String::new();
        //     let mut all_his_pe_sql = String::new();
        //     let mut ses_relates = Vec::new();
        //     let sesno = cur_ses_page.sesno;
        //     // let ses_str = cur_ses_page.get_id(pdms_header.db_num);
        //     for (i, loc) in all_locs.iter().enumerate() {
        //         //如果是最新的，就不需要加版本后缀
        //         //如果是历史版本，就需要有历史后缀，简单点就是是否之后出现过
        //         let refno = loc.get_refno();
        //         let offset = loc.offset;
        //         //从后往前找的天然优势，就是后面的永远是最新的，如果发现历史的数据了，就加上版本号
        //         let is_latest = !latest_refno_map.contains_key(&refno);
        //         let pe_id = if is_latest {
        //             format!("pe:{}", refno)
        //         } else {
        //             format!("pe:['{}',{}]", refno, sesno)
        //         };
        //         // all_relates.push(format!(
        //         //     "{{ id:[e3d_ses:{}, {i}], in: {}, out: e3d_ses:{}, pgno:{}, offset:{} }}",
        //         //     ses_str, &pe_id, ses_str, loc.pgno, loc.offset
        //         // ));
        //         //先暂时不管引用的数据？如果是引用的，需要先按 sesno 查询到当前对应的数据，
        //         //可以用 id 扫描的办法，得到最新的数据？
        //         if let Ok(ele_data) = self.get_element(loc.get_att_offset()).await {
        //             let att = ele_data.att_map();
        //             // all_children_map.entry(refno);
        //             if !is_latest {
        //                 //如果是历史数据，版本号加上
        //                 let json = att
        //                     .gen_sur_json_with_id(format!("['{}',{}]", refno.to_string(), sesno))
        //                     .unwrap();
        //                 let sql = format!("INSERT IGNORE INTO  {}_H {};", att.get_type_str(), &json);
        //                 all_his_att_sql.push_str(&sql);
        //                 let pe_sql = format!(
        //                     "INSERT IGNORE INTO  pe_h {};",
        //                     att.pe(dbnum).gen_sur_json_with_sesno(sesno)
        //                 );
        //                 // println!("pe sql: {}", &pe_sql);
        //                 all_his_pe_sql.push_str(&pe_sql);
        //                 //如果有历史 children 数据，而且 children 数据和当前的不一致，需要列出来哪些是新增的，那些是删除的
        //                 if let Some(old_children) = all_children_map.get(&refno) {
        //                     let mut new_children = &ele_data.children;
        //                     let mut all_deleted = old_children
        //                         .iter()
        //                         .cloned()
        //                         .filter(|x| !new_children.contains(x))
        //                         .collect::<BTreeSet<_>>();
        //                     let mut all_added = new_children
        //                         .iter()
        //                         .cloned()
        //                         .filter(|x| !old_children.contains(x))
        //                         .collect::<BTreeSet<_>>();
        //                     for r in &all_deleted {
        //                         let op: i32 = DataOperation::Deleted.into();
        //                         ses_relates.push(format!("{{ id:[e3d_ses:{}, {}], in: pe:{}, out: e3d_ses:{}, refno: {}, offset:{}, op: {} }}",
        //                                                  sesno, i, r, sesno, refno.to_pe_key(), offset, op));
        //                     }

        //                     for r in &all_added {
        //                         let op: i32 = DataOperation::Added.into();
        //                         ses_relates.push(format!("{{ id:[e3d_ses:{}, {}], in: pe:{}, out: e3d_ses:{}, refno: {}, offset:{}, op: {} }}",
        //                             sesno, i, r, sesno, refno.to_pe_key(), offset, op));
        //                     }

        //                     if !all_deleted.is_empty() || !all_added.is_empty() {
        //                         println!("{sesno} Deleted: {:?}", &all_deleted);
        //                         println!("{sesno} Added: {:?}", &all_added);
        //                     }
        //                 }
        //             }
        //             //如果是最新的数据，就保存起来
        //             all_children_map.insert(refno, ele_data.children);
        //         }
        //         latest_refno_map.entry(refno).or_insert_with(|| loc.clone());
        //     }
        //     // println!("hist att sql: {}", &all_his_att_sql);
        //     //保存历史属性数据
        //     SUL_DB.query(all_his_att_sql).await.unwrap();
        //     SUL_DB.query(all_his_pe_sql).await.unwrap();
        //     println!("会话: {:#4X?} 保存完毕", cur_ses_pgno);

        //     //直接通过数据库查是否最新？还是通过文件查找？
        //     //每个参考号都去拉取一遍，然后看看是不是最新的？

        //     // let offset = cur_ses_page.end_pgno * 0x800 + 0x4;
        //     // let bytes = io.read_bytes(offset, 4).unwrap();
        //     // let type_name = db1_dehash(u32::from_be_bytes(bytes.try_into().unwrap()));
        //     // dbg!(type_name);
        //     // println!("session pgno {:#4X}: {:#4X}", cur_ses_pgno, offset / 0x800);
        //     // dbg!((cur_ses_no, offset));
        //     // dbg!(cur_ses_page.last_ses_pageno);
        //     if step == 50 {
        //         break;
        //     }
        //     if cur_ses_page.last_ses_pageno < 0 {
        //         break;
        //     }
        //     step += 1;
        //     cur_ses_pgno = cur_ses_page.last_ses_pageno as _;
        //     // dbg!(last_ses_no);
        //     // dbg!(cur_ses_page.get_timestamp());
        //     // dbg!(cur_ses_page.get_computer_name());
        //     // dbg!(cur_ses_page.get_comments_name());

        //     // break;
        // }

        // Self::save_ses_pe_relates(&all_relates).await;

        // return Ok(());

        // //refno_pgnos_map 查询里面 value 最多的项
        // // let max_history_refno = refno_pgnos_map.iter().max_by_key(|x| x.1.len());
        // // dbg!(&max_history_refno);

        // //pe_history
        // //pe_owner history 是否有必要
        // //pe_owner 始终是最新的数据
        // //pe_owner_history  为  pe_history 之间的关联关系？也有可能是 pe
        // //如果 children 发生变化，确实需要记录这个，如果 pe 里没有的，那就是在pe_history 里面
        // //NOUN_history
        // //保存历史属性数据到数据库
        // let mut type_att_map = BTreeMap::new();
        // let mut found = false;
        // //e3d_session 是否要绑定一个Operation log 的指向，还是直接可以对比两个session 就可以得到？
        // //但是这样没法实现参考号查询自己是啥时候发生删除的，或者修改的
        // // let mut history_owner_map = HashMap::new();
        // // for (&refno, locs) in &refno_pgnos_map {
        // //     if locs.is_empty(){
        // //         continue;
        // //     }
        // //     //表示有历史记录，后面存储的都是old data, 查询时需要和latest data 合着一起查询
        // //     dbg!(locs.len());
        // //     //（1）按着从小到大的顺序排列的，所以后面的是新的，可以判断构件是否被删除
        // //     //如果是新增加的呢？怎么样维护这个是否新增的关系，这里就要比较这个 ses no 的关系了，在查询的时候，如果是按
        // //     //历史记录查询，需要加个 sesno 的条件过滤，或者 pgno 的过滤，子节点的 pngo 不能超过某个pgno
        // //     //删除了肯定是不能再加回去这个参考号的
        // //     //是否需要弄个pe_history? 还是就放在 pe 里面？应该是都放在 pe 里，然后历史的数据需要加上，以为 pe 是唯一的
        // //     //即使属性发生变化，也只是 pgno 的变化
        // //     let mut prev_children = RefU64Vec::default();
        // //     //todo 使用 chunk
        // //     let mut ses_relates = Vec::new();
        // //     let len = locs.len();
        // //     let mut all_pes = Vec::new();
        // //     //pe_owner 怎么处理？
        // //     //解析时，要快速定位所在 sesno，要记录下来，设置到 pgno，现在不能用 pgno 了，sesno 更具有代表性
        // //     for (index, (pgno, sesno, offset)) in locs.into_iter().enumerate() {
        // //         let addr = *pgno as u64 * 0x800 + *offset as u64 * 2;
        // //         if let Ok(mut data) = self.get_element(addr).await{
        // //             let mut att = &mut data.whole_attmap.attmap;
        // //             let mut pe = att.pe(dbnum);
        // //             //需要在这里检测是否和上一个比，有 delete 的变化，也就是比较 children
        // //             //检查 children 的数据是否发生变化
        // //             if !data.children.is_empty(){
        // //                 //如果在历史层级关系里没有查询到的，需要去 pe 里去找，如果 pe 里没有那就是真没有
        // //                 //保存节点关系的历史记录
        // //                 // let owner_id = pe.history_id();
        // //                 // history_owner_map.insert((pe.refno, pgno, sesno), data.children.clone());
        // //                 //TODO modified refnos
        // //                 //过滤出删除的参考号
        // //                 let all_deleted = prev_children.iter().cloned().filter(|x|{
        // //                     !data.children.contains(x)
        // //                 }).collect::<BTreeSet<_>>();
        // //
        // //                 //过滤出新增的参考号
        // //                 let all_added = data.children.iter().cloned().filter(|x|{
        // //                     !prev_children.contains(x)
        // //                 }).collect::<BTreeSet<_>>();
        // //                 let ses_id = format!("{}_{}_{:0>6}", project, dbnum, sesno);
        // //                 //插入删除的操作记录
        // //                 //将删除的 pe 要重新插入回去，然后设置为 deleted
        // //                 for (j, r) in all_deleted.iter().enumerate(){
        // //                     let op: i32 = DataOperation::Deleted.into();
        // //                     ses_relates.push(format!("{{ id:[e3d_ses:{}, {}], in: pe:{}, out: e3d_ses:{}, op: {} }}",
        // //                                              ses_id, len+j, r, ses_id, op));
        // //                     //要读取到这个删除的 att
        // //                     pe.deleted = true;
        // //                     // all_deleted_pes.push(pe);
        // //                 }
        // //
        // //                 //插入新增的增加的记录
        // //                 for r in &all_added{
        // //                     let op: i32 = DataOperation::Added.into();
        // //                     ses_relates.push(format!("{{ id:[e3d_ses:{}, {}], in: pe:{}, out: e3d_ses:{}, refno: {}, pgno:{}, offset:{}, op: {} }}",
        // //                                              ses_id, index, r, ses_id, refno.to_pe_key(), pgno, offset, op));
        // //                 }
        // //                 //既不是新增，又不是删除，那就是修改
        // //                 if !all_added.contains(&refno) && !all_deleted.contains(&refno) {
        // //                     let op: i32 = DataOperation::Modified.into();
        // //                     ses_relates.push(format!("{{ id:[e3d_ses:{}, {}], in: pe:{}, out: e3d_ses:{}, pgno:{}, offset:{}, op: {} }}",
        // //                                              ses_id, index, refno, ses_id, pgno, offset, op));
        // //                 }
        // //
        // //                 //修改的需要去比对属性数据
        // //
        // //                 if !all_deleted.is_empty() || !all_added.is_empty() {
        // //                     println!("{refno}_{pgno}: {:?}", prev_children);
        // //                     println!("{refno}_{pgno}: {:?}", data.children);
        // //                     println!("Deleted: {:?}", &all_deleted);
        // //                     println!("Added: {:?}", &all_added);
        // //                     //todo 需要将这个 relate 关系加回去，同时创建 pe，并设置为 delete
        // //                     //todo 怎么查询这个构件是什么时候删除的呢，需要关联操作日志
        // //                 }
        // //                 prev_children = data.children.clone();
        // //             }
        // //             // if prev_children == data.children {
        // //             //     //新加的部分也要放在pe_relate里去
        // //             // }else{
        // //             //     prev_children = data.children.clone();
        // //             // }
        // //             all_pes.push(pe);
        // //             type_att_map.entry(att.get_type()).or_insert(Vec::new()).push(data);
        // //         }else{
        // //             dbg!((pgno, offset));
        // //             break;
        // //         }
        // //     }
        // //
        // //
        // //
        // //     if !ses_relates.is_empty(){
        // //         let relate_sql = format!("INSERT RELATION INTO ses_pe_relate [{}];", ses_relates.join(","));
        // //         // let relate_sql = format!("UPSERT RELATION INTO ses_pe_relate [{}];", all_relates.join(","));
        // //         // println!("relates: {}", &relate_sql);
        // //         SUL_DB.query(relate_sql).await.unwrap();
        // //     }
        // //
        // //     for chunk in all_pes.chunks(1000){
        // //         let mut jsons = vec![];
        // //         for pe in chunk{
        // //             jsons.push(pe.gen_sur_json(Some(pe.history_id())));
        // //         }
        // //         let sql = format!("INSERT IGNORE INTO pe_history [{}];", jsons.join(","));
        // //         // println!("insert sql is {}", &sql);
        // //         SUL_DB.query(sql).await.unwrap();
        // //     }
        // //
        // //     if found{
        // //         break;
        // //     }
        // //     // let sql = format!("INSERT IGNORE INTO  {}_history [{}]",ele.
        // //     //                   jsons.join(","));
        // //     // //执行 sql
        // //     // SUL_DB.query(&sql).await.unwrap();
        // //     // }
        // //     // let max_pgno = kv.1.iter().max().unwrap();
        // //     // let eles = self.collect_eles_in_session(*max_pgno).await;
        // //     // println!("refno: {:#4X?}", refno);
        // //     // println!("max_pgno: {:#4X?}", max_pgno);
        // //     // println!("eles: {:#4X?}", eles.len());
        // //     // println!("eles: {:#4X?}", eles);
        // // }
        //pe_owner_history 对pe 进行修正？还是直接存储这个children 关系？
        //先暂时不支持 relate 的历史纪录？还是反过来加入 relate 的 patch？

        // let mut owner_relates = vec![];
        // for ((refno, pgno, sesno), v) in history_owner_map {
        //     let hid = format!("pe_history:{}_{}", refno, pgno);
        //     for (i, child) in v.into_iter().enumerate() {
        //         let mut child_pgno = None;
        //         if let Some(pgnos) = refno_pgnos_map.get(&child) {
        //             // dbg!(child);
        //             //找到目标 refno， FIX 万一引用的 refno 在同一个 sesno 里呢？
        //             for (((p, _, _), (q, s2, _))) in pgnos.iter().tuple_windows() {
        //                 if *pgno >= *p && *pgno < *q {
        //                     //目标 pgno
        //                     let t_pgno = if *sesno == *s2 {
        //                         //同一个 session 里的数据，取最新的 pgno
        //                         q
        //                     } else{
        //                         p
        //                     };
        //                     child_pgno = Some(t_pgno);
        //                     break;
        //                 }
        //             }
        //         };
        //
        //         let child_hid = if let Some(c) = child_pgno{
        //             format!("pe_history:{}_{}", child, c)
        //         }else{
        //             //todo 暂时用其本身的refno，如果没有找到 refno，因为我们现在测试是截断的
        //             format!("pe:{}", child)
        //         };
        //         owner_relates.push(format!("{{ id:[{}, {}], in: {}, out: {} }}",
        //                                    &hid, i, child_hid, &hid));
        //     }
        // }
        // dbg!(&owner_relates);
        // if !owner_relates.is_empty() {
        //     let relate_sql = format!("INSERT RELATION INTO pe_owner_history [{}];", owner_relates.join(","));
        //     // println!("owner relates: {}", &relate_sql);
        //     SUL_DB.query(relate_sql).await.unwrap();
        // }

        // Self::save_att_history(&mut type_att_map).await;

        Ok(())
    }

    async fn save_ses_pe_relates(all_relates: &Vec<String>) {
        // 修改、删除、增加，放在这里去加一个字段
        for chunk in all_relates.chunks(1000) {
            let relate_sql = format!("INSERT RELATION INTO ses_pe_relate [{}];", chunk.join(","));
            // println!("relates: {}", chunk.join(","));
            SUL_DB.query(relate_sql).await.unwrap();
        }
    }

    async fn save_att_history(type_att_map: &mut BTreeMap<String, Vec<EleData>>) {
        //对 type_att_map 进行历史数据的保存
        //todo 后续可以改解析，都是用这个方法去保存数据, 存属性时，都是用的最新的 sesno
        //只有pe_owner_hsitory 需要用历史的查询？
        //历史数据放到一个表里面，然后通过 id 去查找？
        for (type_name, ele_datas) in type_att_map.into_iter() {
            for es in ele_datas.chunks(1000) {
                let mut jsons = Vec::new();
                //pe 直接就加在 pe_relate，然后通过 pe_relate 去查看 pe 的 delete 属性
                //delete 属性后面要用起来
                for ele_data in es {
                    let id = ele_data.whole_attmap.att_map().history_id();
                    if let Some(json) = ele_data.whole_attmap.att_map().gen_sur_json_with_id(id) {
                        jsons.push(json);
                    }
                }
                let sql = format!(
                    "INSERT IGNORE INTO  {}_history [{}]",
                    type_name,
                    jsons.join(",")
                );
                SUL_DB.query(&sql).await.unwrap();
            }
        }
    }

    //todo 对比两个 session，发生了哪些变化
    //old, new
    pub async fn compare_eles_between_sessions() {}

    #[inline]
    pub fn get_ses_pageno(&self, sesno: i32) -> Option<u32> {
        self.sesno_pgno_map.get(&sesno).cloned()
    }

    #[inline]
    pub fn collect_refno_locs(&mut self, sesno: i32) -> Vec<RefnoDataLoc> {
        self.get_ses_pageno(sesno)
            .map(|ses_pgno| self.collect_refno_locs_in_session(ses_pgno))
            .unwrap_or_default()
    }

    pub fn collect_refno_locs_in_session(&mut self, ses_pgno: u32) -> Vec<RefnoDataLoc> {
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
        self.filter_index_data(
            &index_data,
            &mut final_locs,
            last_end_pgno,
            cur_end_pgno,
            &mut level,
        );

        final_locs
    }

    ///收集一个会话里面的所有的属性数据
    pub async fn collect_eles_in_session(&mut self, ses_pgno: u32) -> Vec<EleData> {
        let final_locs = self.collect_refno_locs_in_session(ses_pgno);
        let mut eles = vec![];
        //根据这个RefnoDataLoc 读取到所有发生更新的 index 数据
        for loc in final_locs {
            let ele = self.get_element(loc.get_att_offset()).await.unwrap();
            eles.push(ele);
        }
        eles
    }

    ///过滤 index page data 里面的数据
    pub fn filter_index_data(
        &mut self,
        index_data: &IndexPageData,
        result_locs: &mut Vec<RefnoDataLoc>,
        last_end_pgno: u32,
        cur_end_pgno: u32,
        level: &mut i32,
    ) -> Option<bool> {
        // let mut level = index_data?.level as i32;
        if index_data.refno_locs.is_empty() {
            return None;
        }
        let cur_locs = index_data
            .refno_locs
            .iter()
            .filter(|x| x.pgno > last_end_pgno && x.pgno < cur_end_pgno && x.flag == 1)
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
                        self.filter_index_data(
                            &next_index_data,
                            result_locs,
                            last_end_pgno,
                            cur_end_pgno,
                            &mut next_level,
                        );
                    }
                }
            }
        }
        Some(true)
    }

    /// 收集session范围内的增删改的element数据
    /// 并在这里即可判断是否增删改？
    pub async fn collect_increment_eles(
        &mut self,
        sesno_range: RangeInclusive<i32>,
    ) -> anyhow::Result<HashMap<RefU64, EleData>> {
        let mut eles_map = HashMap::new();

        for sesno in sesno_range.into_iter().rev() {
            let final_locs = self.collect_refno_locs(sesno);
            //从后往前查看，如果是已经有了数据，就不需要再往里面加了
            for loc in final_locs {
                if !eles_map.contains_key(&loc.get_refno()) {
                    let ele = self.get_element(loc.get_att_offset()).await?;
                    eles_map.insert(ele.refno, ele);
                }
            }
        }
        Ok(eles_map)
    }

    //直接读取中间这段数据的att index table，直接获取所有需要的数据
    pub async fn collect_increment_eles_old(
        &mut self,
        till_pageno: u32,
    ) -> anyhow::Result<HashMap<RefU64, EleData>> {
        let mut file = self.get_file()?;
        let mut input = vec![];
        let start = till_pageno as u64 * 0x800;
        #[cfg(feature = "debug_parse")]
        println!("Bytes start at : {:#04X?}", start);
        file.seek(SeekFrom::Start(start))
            .expect("collect_increment_eles");
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
                println!(
                    "Found loc: {:#04X?}, att_offset: {:#04X}",
                    &x, refno_att_offset
                );
                let refno = RefU64::from_two_nums(x.refno_0, x.refno_1);
                if !refno_data_offsets_map.contains_key(&refno) {
                    refno_data_offsets_map.insert(refno, refno_att_offset);
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

pub async fn sync_all_history_data(path: &str) -> anyhow::Result<()> {
    //先建立 ses 的索引，date 和 dbnum， sesno 都要建立索引
    let mut io = PdmsIO::new("ams", path, true);
    io.sync_history().await.unwrap();
    Ok(())
}
