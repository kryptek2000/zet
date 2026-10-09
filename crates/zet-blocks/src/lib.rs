use rusqlite::{params, Connection, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub id: i64,
    pub session: String,
    pub command: String,
    pub output: String,
    pub exit_code: i32,
    pub cwd: String,
    pub duration_ms: u64,
}

pub struct Store {
    conn: Connection,
}

impl Store {
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let store = Store { conn };
        store.init()?;
        Ok(store)
    }

    pub fn open(path: &str) -> Result<Self> {
        let conn = Connection::open(path)?;
        let store = Store { conn };
        store.init()?;
        Ok(store)
    }

    fn init(&self) -> Result<()> {
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS blocks (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                session     TEXT NOT NULL,
                command     TEXT NOT NULL,
                output      TEXT NOT NULL DEFAULT '',
                exit_code   INTEGER NOT NULL DEFAULT 0,
                cwd         TEXT NOT NULL DEFAULT '',
                duration_ms INTEGER NOT NULL DEFAULT 0
            )",
            (),
        )?;
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_blocks_session ON blocks (session, id)",
            (),
        )?;
        Ok(())
    }

    pub fn append(&self, block: &Block) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO blocks (session, command, output, exit_code, cwd, duration_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                block.session,
                block.command,
                block.output,
                block.exit_code,
                block.cwd,
                block.duration_ms as i64,
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn get(&self, id: i64) -> Result<Block> {
        self.conn.query_row(
            "SELECT id, session, command, output, exit_code, cwd, duration_ms
             FROM blocks WHERE id = ?1",
            params![id],
            row_to_block,
        )
    }

    pub fn list(&self, session: &str) -> Result<Vec<Block>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, session, command, output, exit_code, cwd, duration_ms
             FROM blocks WHERE session = ?1 ORDER BY id ASC",
        )?;
        let rows = stmt.query_map(params![session], row_to_block)?;
        rows.collect()
    }

    pub fn replay(&self, session: &str) -> Result<String> {
        let mut out = String::new();
        for block in self.list(session)? {
            out.push_str("$ ");
            out.push_str(&block.command);
            out.push('\n');
            out.push_str(&block.output);
            out.push_str(&format!(
                "[exit {}, {}ms]\n",
                block.exit_code, block.duration_ms
            ));
        }
        Ok(out)
    }
}

fn row_to_block(row: &rusqlite::Row<'_>) -> Result<Block> {
    Ok(Block {
        id: row.get(0)?,
        session: row.get(1)?,
        command: row.get(2)?,
        output: row.get(3)?,
        exit_code: row.get(4)?,
        cwd: row.get(5)?,
        duration_ms: row.get::<_, i64>(6)? as u64,
    })
}
