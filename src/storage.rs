use color_eyre::eyre::Context;
use color_eyre::Result;
use chrono::Local;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Memo {
    pub path: PathBuf,
    pub content: String,
    // We derive metadata from file system
}

impl Memo {
    pub fn new(content: String) -> Self {
        let now = Local::now();
        let root = get_root_dir();
        
        // format: tui-memo/YYYY-MM-DD/HH-mm-ss.md
        let date_part = now.format("%Y-%m-%d").to_string();
        let time_part = now.format("%H-%M-%S.md").to_string();
        
        let path = root.join(date_part).join(time_part);

        Self {
            path,
            content,
        }
    }

    pub fn title(&self) -> String {
        self.content
            .lines()
            .next()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "New Memo".to_string())
    }

    pub fn save(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).context("Failed to create parent directories")?;
        }
        fs::write(&self.path, &self.content).context("Failed to write memo file")?;
        Ok(())
    }

    pub fn delete(&self) -> Result<()> {
        if self.path.exists() {
            fs::remove_file(&self.path).context("Failed to delete memo file")?;
        }
        Ok(())
    }
}

pub fn load_memos() -> Result<Vec<Memo>> {
    let root = get_root_dir();
    if !root.exists() {
        return Ok(Vec::new());
    }

    let mut memos = Vec::new();
    // 2-level traversal: root -> date_dir -> memo_file
    for entry in fs::read_dir(&root).context("Failed to read root directory")? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            for sub_entry in fs::read_dir(&path)? {
                let sub_entry = sub_entry?;
                let sub_path = sub_entry.path();
                if sub_path.extension().map_or(false, |ext| ext == "md") {
                    let content = fs::read_to_string(&sub_path)?;
                    memos.push(Memo {
                        path: sub_path,
                        content,
                    });
                }
            }
        }
    }
    // Sort items by path (effectively date due to naming convention) descending
    memos.sort_by(|a, b| b.path.cmp(&a.path));
    Ok(memos)
}

fn get_root_dir() -> PathBuf {
    #[cfg(test)]
    {
         std::env::current_dir().unwrap().join("test_tui_memo")
    }
    #[cfg(not(test))]
    {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        home.join("tui-memo")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memo_title_extraction() {
        let memo = Memo {
            path: PathBuf::from("dummy"),
            content: "Title\nBody content".to_string(),
        };
        assert_eq!(memo.title(), "Title");

        let memo_empty = Memo {
            path: PathBuf::from("dummy"),
            content: "".to_string(),
        };
        assert_eq!(memo_empty.title(), "New Memo");
    }

    #[test]
    fn test_storage_lifecycle() -> Result<()> {
        let root = get_root_dir();
        if root.exists() {
            fs::remove_dir_all(&root)?;
        }

        // 1. Create and Save
        let memo = Memo::new("Test Title\nContent".to_string());
        memo.save()?;

        assert!(memo.path.exists());

        // 2. Load
        let memos = load_memos()?;
        assert_eq!(memos.len(), 1);
        assert_eq!(memos[0].content, "Test Title\nContent");
        assert_eq!(memos[0].path, memo.path);

        // 3. Delete
        memo.delete()?;
        assert!(!memo.path.exists());

        // 4. Load again
        let memos_after = load_memos()?;
        assert!(memos_after.is_empty());

        // Cleanup
        if root.exists() {
            fs::remove_dir_all(&root)?;
        }
        Ok(())
    }
}
