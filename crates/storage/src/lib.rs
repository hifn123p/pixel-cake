//! `storage` — 数据层（文档 §2 第④层、§6 SQLite Schema）。
//!
//! 元数据（项目/照片/recipe/预设/模型）入 SQLite，库静态编译进二进制，无外部服务。
//! 大文件（原图/代理/成品/蒙版/模型/LUT）存本地目录，SQLite 仅存路径与元数据，
//! 避免库膨胀。
//!
//! 注：`rusqlite::Connection` 非 `Sync`，故以 `Mutex` 包裹，使 `Store` 满足
//! Tauri `State` 要求的 `Send + Sync`。

use std::path::Path;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use bus::Recipe;
use rusqlite::{params, Connection, Result as SqlResult};
use serde::Serialize;
use thiserror::Error;

/// 项目（客片组）。
#[derive(Debug, Clone, Serialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub created_at: i64,
    pub root_path: String,
    pub thumb: Option<String>,
}

/// 照片处理状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PhotoStatus {
    Pending,
    Retouched,
    Exported,
}

impl PhotoStatus {
    fn as_str(self) -> &'static str {
        match self {
            PhotoStatus::Pending => "pending",
            PhotoStatus::Retouched => "retouched",
            PhotoStatus::Exported => "exported",
        }
    }
    fn from_str(s: &str) -> Self {
        match s {
            "retouched" => PhotoStatus::Retouched,
            "exported" => PhotoStatus::Exported,
            _ => PhotoStatus::Pending,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Photo {
    pub id: String,
    pub project_id: String,
    pub raw_path: String,
    pub proxy_path: Option<String>,
    pub result_path: Option<String>,
    pub width: u32,
    pub height: u32,
    pub status: PhotoStatus,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Preset {
    pub id: String,
    pub name: String,
    /// 推荐 / 我的样片。
    pub scope: String,
    pub recipe_json: String,
    pub lut_path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelMeta {
    pub id: String,
    pub name: String,
    pub path: String,
    pub version: String,
    pub hash: String,
    pub enabled: bool,
}

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("recipe 序列化失败: {0}")]
    Serialize(#[from] serde_json::Error),
    #[error("未找到照片: {0}")]
    NotFound(String),
}

pub type Result<T> = std::result::Result<T, StorageError>;

/// SQLite 数据访问门面。
pub struct Store {
    conn: Mutex<Connection>,
}

impl Store {
    /// 打开（不存在则创建）数据库并初始化 schema。
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// 内存库（测试用）。
    pub fn in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn create_project(&self, name: &str, root_path: &str) -> Result<Project> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = now_secs();
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO project(id, name, created_at, root_path) VALUES (?1, ?2, ?3, ?4)",
            params![id, name, now, root_path],
        )?;
        Ok(Project {
            id,
            name: name.to_string(),
            created_at: now,
            root_path: root_path.to_string(),
            thumb: None,
        })
    }

    pub fn list_projects(&self) -> Result<Vec<Project>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, created_at, root_path, thumb FROM project ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(Project {
                id: r.get(0)?,
                name: r.get(1)?,
                created_at: r.get(2)?,
                root_path: r.get(3)?,
                thumb: r.get(4)?,
            })
        })?;
        rows.collect::<SqlResult<Vec<_>>>().map_err(Into::into)
    }

    pub fn import_photos(
        &self,
        project_id: &str,
        paths: &[String],
        dims: impl Fn(&str) -> (u32, u32),
    ) -> Result<Vec<Photo>> {
        let mut out = Vec::with_capacity(paths.len());
        let now = now_secs();
        let conn = self.conn.lock().unwrap();
        for p in paths {
            let id = uuid::Uuid::new_v4().to_string();
            let (w, h) = dims(p);
            conn.execute(
                "INSERT INTO photo(id, project_id, raw_path, width, height, status, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, 'pending', ?6)",
                params![id, project_id, p, w, h, now],
            )?;
            out.push(Photo {
                id,
                project_id: project_id.to_string(),
                raw_path: p.clone(),
                proxy_path: None,
                result_path: None,
                width: w,
                height: h,
                status: PhotoStatus::Pending,
                created_at: now,
            });
        }
        Ok(out)
    }

    pub fn list_photos(&self, project_id: &str) -> Result<Vec<Photo>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, project_id, raw_path, proxy_path, result_path, width, height, status, created_at FROM photo WHERE project_id = ?1",
        )?;
        let rows = stmt.query_map(params![project_id], |r| Ok(row_to_photo(r)?))?;
        rows.collect::<SqlResult<Vec<_>>>().map_err(Into::into)
    }

    pub fn get_photo(&self, photo_id: &str) -> Result<Option<Photo>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, project_id, raw_path, proxy_path, result_path, width, height, status, created_at FROM photo WHERE id = ?1",
        )?;
        let mut rows = stmt.query(params![photo_id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row_to_photo(row)?))
        } else {
            Ok(None)
        }
    }

    pub fn update_photo_status(&self, photo_id: &str, status: PhotoStatus) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE photo SET status = ?1 WHERE id = ?2",
            params![status.as_str(), photo_id],
        )?;
        Ok(())
    }

    pub fn save_recipe(&self, photo_id: &str, recipe: &Recipe) -> Result<()> {
        let data = recipe.to_json();
        let now = now_secs();
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO recipe(photo_id, data, updated_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(photo_id) DO UPDATE SET data = excluded.data, updated_at = excluded.updated_at",
            params![photo_id, data, now],
        )?;
        Ok(())
    }

    pub fn get_recipe(&self, photo_id: &str) -> Result<Option<Recipe>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT data FROM recipe WHERE photo_id = ?1")?;
        let mut rows = stmt.query(params![photo_id])?;
        if let Some(row) = rows.next()? {
            let data: String = row.get(0)?;
            Ok(Some(Recipe::from_json(&data)?))
        } else {
            Ok(None)
        }
    }

    pub fn save_preset(
        &self,
        name: &str,
        scope: &str,
        recipe: &Recipe,
        lut_path: Option<&str>,
    ) -> Result<Preset> {
        let id = uuid::Uuid::new_v4().to_string();
        let recipe_json = recipe.to_json();
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO preset(id, name, scope, recipe_json, lut_path) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, name, scope, recipe_json, lut_path],
        )?;
        Ok(Preset {
            id,
            name: name.to_string(),
            scope: scope.to_string(),
            recipe_json,
            lut_path: lut_path.map(str::to_string),
        })
    }

    pub fn list_presets(&self) -> Result<Vec<Preset>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT id, name, scope, recipe_json, lut_path FROM preset")?;
        let rows = stmt.query_map([], |r| {
            Ok(Preset {
                id: r.get(0)?,
                name: r.get(1)?,
                scope: r.get(2)?,
                recipe_json: r.get(3)?,
                lut_path: r.get(4)?,
            })
        })?;
        rows.collect::<SqlResult<Vec<_>>>().map_err(Into::into)
    }

    pub fn register_model(&self, m: &ModelMeta) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO model(id, name, path, version, hash, enabled) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(id) DO UPDATE SET version = excluded.version, hash = excluded.hash, enabled = excluded.enabled",
            params![m.id, m.name, m.path, m.version, m.hash, m.enabled as i32],
        )?;
        Ok(())
    }
}

fn row_to_photo(r: &rusqlite::Row<'_>) -> SqlResult<Photo> {
    Ok(Photo {
        id: r.get(0)?,
        project_id: r.get(1)?,
        raw_path: r.get(2)?,
        proxy_path: r.get(3)?,
        result_path: r.get(4)?,
        width: r.get(5)?,
        height: r.get(6)?,
        status: PhotoStatus::from_str(&r.get::<_, String>(7)?),
        created_at: r.get(8)?,
    })
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// 文档 §6 SQLite Schema 草案。
const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS project(
  id TEXT PRIMARY KEY, name TEXT, created_at INTEGER,
  root_path TEXT, thumb TEXT);
CREATE TABLE IF NOT EXISTS photo(
  id TEXT PRIMARY KEY, project_id TEXT,
  raw_path TEXT, proxy_path TEXT, result_path TEXT,
  width INTEGER, height INTEGER, status TEXT,
  created_at INTEGER);
CREATE TABLE IF NOT EXISTS recipe(
  photo_id TEXT PRIMARY KEY,
  data TEXT,
  updated_at INTEGER);
CREATE TABLE IF NOT EXISTS preset(
  id TEXT PRIMARY KEY, name TEXT, scope TEXT,
  recipe_json TEXT, lut_path TEXT);
CREATE TABLE IF NOT EXISTS model(
  id TEXT PRIMARY KEY, name TEXT, path TEXT,
  version TEXT, hash TEXT, enabled INTEGER);
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_and_recipe_roundtrip() {
        let s = Store::in_memory().unwrap();
        let p = s.create_project("客片 A", "C:/lib/A").unwrap();
        let photos = s
            .import_photos(&p.id, &["C:/lib/A/raw/1.cr2".into()], |_| (4000, 6000))
            .unwrap();
        assert_eq!(photos.len(), 1);

        let mut recipe = Recipe::default();
        recipe.neutral_gray.enabled = true;
        recipe.neutral_gray.ka = 60;
        s.save_recipe(&photos[0].id, &recipe).unwrap();

        let got = s.get_recipe(&photos[0].id).unwrap().unwrap();
        assert_eq!(got.neutral_gray.ka, 60);
    }
}
