use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::fs::{File, Metadata};
use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};

use aios_core::pdms_types::RefU64;
use aios_core::tool::db_tool::db1_dehash;

use crate::io::PdmsIO;
use aios_core::db::DbBasicData;

#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
#[cfg(windows)]
use std::os::windows::fs::MetadataExt;

#[derive(Debug, Clone, PartialEq, Eq)]
enum StableFileId {
    #[cfg(windows)]
    Windows { volume_serial: u32, file_index: u64 },
    #[cfg(unix)]
    Unix { device: u64, inode: u64 },
}

impl StableFileId {
    fn from_metadata(metadata: &Metadata) -> anyhow::Result<Self> {
        #[cfg(windows)]
        {
            let volume_serial = metadata
                .volume_serial_number()
                .ok_or_else(|| anyhow::anyhow!("文件元数据缺少 volume serial"))?;
            let file_index = metadata
                .file_index()
                .ok_or_else(|| anyhow::anyhow!("文件元数据缺少 file index"))?;
            return Ok(Self::Windows {
                volume_serial,
                file_index,
            });
        }
        #[cfg(unix)]
        {
            return Ok(Self::Unix {
                device: metadata.dev(),
                inode: metadata.ino(),
            });
        }
        #[allow(unreachable_code)]
        Err(anyhow::anyhow!("当前平台没有 dabacon 稳定文件身份实现"))
    }
}

/// Proof that metadata, header and session state came from one open file handle.
///
/// Fields are private so callers cannot assemble a token from independent path
/// reads. Path/length are diagnostics; `file_id` is the replacement guard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotToken {
    path: PathBuf,
    file_id: StableFileId,
    dbnum: i32,
    db_type: String,
    target_sesno: u32,
    latest_ses_pgno: u32,
    opened_len: u64,
    header_prefix: Vec<u8>,
}

impl SnapshotToken {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn dbnum(&self) -> i32 {
        self.dbnum
    }

    pub fn db_type(&self) -> &str {
        &self.db_type
    }

    pub fn target_sesno(&self) -> u32 {
        self.target_sesno
    }

    pub fn opened_len(&self) -> u64 {
        self.opened_len
    }

    pub fn latest_ses_pgno(&self) -> u32 {
        self.latest_ses_pgno
    }

    #[cfg(test)]
    pub(crate) fn for_test() -> Self {
        Self {
            path: PathBuf::from("test.db"),
            #[cfg(windows)]
            file_id: StableFileId::Windows {
                volume_serial: 1,
                file_index: 1,
            },
            #[cfg(unix)]
            file_id: StableFileId::Unix {
                device: 1,
                inode: 1,
            },
            dbnum: 1,
            db_type: "DESI".to_owned(),
            target_sesno: 30,
            latest_ses_pgno: 1,
            opened_len: 0,
            header_prefix: vec![0; 60],
        }
    }
}

/// Authoritative dabacon snapshot backed by one open file handle.
pub struct DabaconSnapshot {
    io: PdmsIO,
    token: SnapshotToken,
}

impl DabaconSnapshot {
    pub fn open(project: impl Into<String>, path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let path = path.as_ref().to_path_buf();
        let mut io = PdmsIO::new(project.into(), &path, true);
        io.open().map_err(|error| {
            anyhow::anyhow!("打开 dabacon 快照 {} 失败: {error}", path.display())
        })?;

        let metadata = io.opened_file_metadata()?;
        let file_id = StableFileId::from_metadata(&metadata)?;
        let header_prefix = io.read_exact_prefix_from_opened_file(60)?;
        let header = io.read_pdms_header()?;
        let target_sesno = io.get_latest_sesno()?;
        io.dbnum = header.db_num;

        let token = SnapshotToken {
            path,
            file_id,
            dbnum: header.db_num,
            db_type: db1_dehash(header.noun as u32).to_ascii_uppercase(),
            target_sesno,
            latest_ses_pgno: header.latest_ses_pgno,
            opened_len: metadata.len(),
            header_prefix,
        };
        Ok(Self { io, token })
    }

    /// Open the current file generation but freeze an explicit historical root.
    pub fn open_at(
        project: impl Into<String>,
        path: impl AsRef<Path>,
        target_sesno: u32,
    ) -> anyhow::Result<Self> {
        let mut snapshot = Self::open(project, path)?;
        snapshot.freeze_target(target_sesno)?;
        Ok(snapshot)
    }

    fn freeze_target(&mut self, target_sesno: u32) -> anyhow::Result<()> {
        let pgno = self
            .io
            .sesno_pgno_map
            .get(&(target_sesno as i32))
            .copied()
            .ok_or_else(|| anyhow::anyhow!("dabacon 快照缺少目标会话 {target_sesno}"))?;
        anyhow::ensure!(
            target_sesno <= self.token.target_sesno,
            "请求冻结会话 {target_sesno} 超过文件权威会话 {}",
            self.token.target_sesno
        );
        self.token.target_sesno = target_sesno;
        self.token.latest_ses_pgno = pgno;
        Ok(())
    }

    /// Re-open a path and prove that it is the same file generation as `token`.
    /// A later appended session is accepted, while the requested target remains
    /// the frozen session stored in `token`.
    pub fn open_verified(
        project: impl Into<String>,
        token: &SnapshotToken,
    ) -> anyhow::Result<Self> {
        let mut snapshot = Self::open(project, &token.path)?;
        anyhow::ensure!(
            snapshot.token.file_id == token.file_id,
            "dabacon 路径 {} 的文件身份与冻结 token 不一致",
            token.path.display()
        );
        anyhow::ensure!(
            snapshot.token.dbnum == token.dbnum && snapshot.token.db_type == token.db_type,
            "dabacon 文件身份字段漂移：冻结 dbnum/type={}/{}, 当前={}/{}",
            token.dbnum,
            token.db_type,
            snapshot.token.dbnum,
            snapshot.token.db_type
        );
        anyhow::ensure!(
            snapshot.token.target_sesno >= token.target_sesno,
            "dabacon 会话从冻结值 {} 回退到 {}",
            token.target_sesno,
            snapshot.token.target_sesno
        );
        anyhow::ensure!(
            snapshot.token.opened_len >= token.opened_len,
            "dabacon 文件长度从冻结值 {} 回退到 {}",
            token.opened_len,
            snapshot.token.opened_len
        );
        anyhow::ensure!(
            snapshot
                .io
                .sesno_pgno_map
                .contains_key(&(token.target_sesno as i32)),
            "dabacon 当前世代缺少冻结会话 {}",
            token.target_sesno
        );
        // Keep the original target, length and header bytes. The reopened handle
        // may expose later appended sessions, but consumers must remain bound to
        // the generation represented by the frozen token.
        snapshot.token = token.clone();
        Ok(snapshot)
    }

    /// Re-open the same file generation and narrow it to a target no newer than
    /// the original freeze point (used by max-session window splitting).
    pub fn open_verified_at(
        project: impl Into<String>,
        token: &SnapshotToken,
        target_sesno: u32,
    ) -> anyhow::Result<Self> {
        anyhow::ensure!(
            target_sesno <= token.target_sesno,
            "请求会话 {target_sesno} 超过冻结 token target={} ",
            token.target_sesno
        );
        let mut snapshot = Self::open_verified(project, token)?;
        snapshot.freeze_target(target_sesno)?;
        Ok(snapshot)
    }

    pub fn token(&self) -> &SnapshotToken {
        &self.token
    }

    /// Verify that the path still names the file opened by this snapshot.
    /// File growth is accepted; replacement by another file is rejected.
    pub fn verify_path_identity(&self) -> anyhow::Result<()> {
        let metadata = File::open(&self.token.path)?.metadata()?;
        let actual = StableFileId::from_metadata(&metadata)?;
        anyhow::ensure!(
            actual == self.token.file_id,
            "dabacon 路径 {} 已指向另一文件（冻结身份不匹配）",
            self.token.path.display()
        );
        Ok(())
    }

    pub fn contains_refnos_at(
        &mut self,
        target_sesno: u32,
        candidates: &BTreeSet<RefU64>,
    ) -> anyhow::Result<BTreeSet<RefU64>> {
        anyhow::ensure!(
            target_sesno <= self.token.target_sesno,
            "请求会话 {target_sesno} 超过快照冻结会话 {}",
            self.token.target_sesno
        );
        self.verify_path_identity()?;
        crate::session_index_diff::contains_refnos_at(&mut self.io, target_sesno as i32, candidates)
    }

    /// Membership audit bound to the same verified file generation as this snapshot.
    pub fn member_alive_at(
        &mut self,
        refno: aios_core::pdms_types::RefU64,
        target_sesno: i32,
    ) -> anyhow::Result<bool> {
        anyhow::ensure!(
            target_sesno >= 0 && target_sesno as u32 <= self.token.target_sesno,
            "membership target sesno {target_sesno} exceeds frozen target {}",
            self.token.target_sesno
        );
        self.verify_path_identity()?;
        crate::net_window::member_alive_at(&mut self.io, refno, target_sesno)
    }

    /// Expand deleted membership roots without reopening the dabacon path.
    pub fn expand_deleted_membership_roots(
        &mut self,
        roots: &std::collections::BTreeSet<aios_core::pdms_types::RefU64>,
        base_sesno: i32,
        target_sesno: i32,
    ) -> anyhow::Result<std::collections::BTreeSet<aios_core::pdms_types::RefU64>> {
        anyhow::ensure!(
            target_sesno >= 0 && target_sesno as u32 <= self.token.target_sesno,
            "membership target sesno {target_sesno} exceeds frozen target {}",
            self.token.target_sesno
        );
        self.verify_path_identity()?;
        crate::net_window::expand_deleted_membership_roots(
            &mut self.io,
            roots,
            base_sesno,
            target_sesno,
        )
    }

    pub fn session_sesnos_in_range(&self, range: RangeInclusive<i32>) -> Vec<u32> {
        self.io
            .sesno_pgno_map
            .range(range)
            .filter_map(|(&sesno, _)| u32::try_from(sesno).ok())
            .collect()
    }

    pub fn session_ranges(&self) -> BTreeMap<i32, std::ops::Range<u32>> {
        self.io.ses_range_map.clone()
    }

    /// Parse a full baseline from the same handle that produced the token.
    pub fn read_full_basic_data(
        &mut self,
        file_name: &str,
        project: &str,
    ) -> anyhow::Result<DbBasicData> {
        self.verify_path_identity()?;
        let mut bytes = self
            .io
            .read_exact_prefix_from_opened_file(self.token.opened_len)?;
        anyhow::ensure!(
            bytes.len() >= self.token.header_prefix.len(),
            "冻结 dabacon 长度 {} 小于文件头 {}",
            bytes.len(),
            self.token.header_prefix.len()
        );
        bytes[..self.token.header_prefix.len()].copy_from_slice(&self.token.header_prefix);
        let basic = parse_pdms_db::parse::parse_db_basic_data(bytes, file_name, project)?;
        self.verify_path_identity()?;
        Ok(basic)
    }

    pub(crate) fn io_mut(&mut self) -> &mut PdmsIO {
        &mut self.io
    }
}

#[cfg(test)]
mod tests {
    use super::StableFileId;
    use crate::io::read_exact_prefix_from_file;
    use std::fs::{self, File, OpenOptions};
    use std::io::Write;
    use std::path::PathBuf;

    fn temp_dir(case: &str) -> PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "pdms-io-snapshot-{case}-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn stable_identity_accepts_append_to_the_same_file() {
        let dir = temp_dir("append");
        let path = dir.join("append.db");
        File::create(&path).unwrap().write_all(b"before").unwrap();
        let before = StableFileId::from_metadata(&fs::metadata(&path).unwrap()).unwrap();
        let frozen_len = fs::metadata(&path).unwrap().len();
        OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(b"after")
            .unwrap();
        let after = StableFileId::from_metadata(&fs::metadata(&path).unwrap()).unwrap();
        assert_eq!(before, after);
        let frozen = read_exact_prefix_from_file(&mut File::open(&path).unwrap(), frozen_len)
            .expect("read frozen prefix");
        assert_eq!(frozen, b"before");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn stable_identity_rejects_atomic_path_replacement() {
        let dir = temp_dir("replace");
        let path = dir.join("current.db");
        let replacement = dir.join("replacement.db");
        File::create(&path).unwrap().write_all(b"first").unwrap();
        File::create(&replacement)
            .unwrap()
            .write_all(b"second")
            .unwrap();
        let before = StableFileId::from_metadata(&fs::metadata(&path).unwrap()).unwrap();
        fs::remove_file(&path).unwrap();
        fs::rename(&replacement, &path).unwrap();
        let after = StableFileId::from_metadata(&fs::metadata(&path).unwrap()).unwrap();
        assert_ne!(before, after);
        fs::remove_dir_all(dir).unwrap();
    }
}
