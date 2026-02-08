use std::collections::VecDeque;
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct RaftEnvelope<E> {
    pub index: u64,
    pub ms: u64,
    pub entry: E,
}

#[derive(Clone, Debug)]
pub struct RaftLog<E> {
    path: PathBuf,
    next_index: u64,
    // Small in-memory tail for in-game `raft tail` without re-reading the file each time.
    recent: VecDeque<String>,
    recent_cap: usize,
    _phantom: std::marker::PhantomData<E>,
}

impl<E> RaftLog<E>
where
    E: serde::Serialize + for<'de> serde::Deserialize<'de> + Clone,
{
    pub fn open(path: PathBuf) -> anyhow::Result<(Self, Vec<RaftEnvelope<E>>)> {
        let mut out = Self {
            path,
            next_index: 1,
            recent: VecDeque::new(),
            recent_cap: 200,
            _phantom: std::marker::PhantomData,
        };
        let replay = out.load_replay()?;
        Ok((out, replay))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn next_index(&self) -> u64 {
        self.next_index
    }

    pub fn recent_lines(&self, n: usize) -> Vec<String> {
        let n = n.max(1).min(self.recent.len());
        self.recent
            .iter()
            .rev()
            .take(n)
            .cloned()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect()
    }

    pub fn append(&mut self, ms: u64, entry: E) -> anyhow::Result<RaftEnvelope<E>> {
        let env = RaftEnvelope {
            index: self.next_index,
            ms,
            entry,
        };
        self.next_index = self.next_index.saturating_add(1);

        if let Some(dir) = self.path.parent() {
            if !dir.as_os_str().is_empty() {
                std::fs::create_dir_all(dir)?;
            }
        }

        let line = serde_json::to_string(&env)?;
        {
            let mut f = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.path)?;
            f.write_all(line.as_bytes())?;
            f.write_all(b"\n")?;
            f.flush()?;
        }

        self.push_recent(line);
        Ok(env)
    }

    fn push_recent(&mut self, line: String) {
        while self.recent.len() >= self.recent_cap {
            self.recent.pop_front();
        }
        self.recent.push_back(line);
    }

    fn load_replay(&mut self) -> anyhow::Result<Vec<RaftEnvelope<E>>> {
        let f = match std::fs::File::open(&self.path) {
            Ok(v) => v,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
            Err(e) => return Err(e.into()),
        };
        let rd = BufReader::new(f);

        let mut out = Vec::new();
        let mut max_index = 0u64;
        for (lineno, line) in rd.lines().enumerate() {
            let line = line?;
            let raw = line.trim();
            if raw.is_empty() {
                continue;
            }
            let env: RaftEnvelope<E> = match serde_json::from_str(raw) {
                Ok(v) => v,
                Err(e) => {
                    return Err(anyhow::anyhow!(
                        "raft log parse error at {}:{}: {}",
                        self.path.display(),
                        lineno + 1,
                        e
                    ));
                }
            };
            max_index = max_index.max(env.index);
            // Keep a tail of lines for quick in-game debugging.
            self.push_recent(raw.to_string());
            out.push(env);
        }
        self.next_index = max_index.saturating_add(1).max(1);
        Ok(out)
    }
}
