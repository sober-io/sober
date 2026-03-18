//! Filesystem-based skill discovery with TTL caching.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use tokio::fs;
use tracing::{debug, warn};

use crate::SkillError;
use crate::catalog::SkillCatalog;
use crate::frontmatter::{parse_skill_frontmatter, validate_skill_name};
use crate::types::{SkillEntry, SkillSource};

const MAX_SCAN_DIRS: usize = 2000;

struct CachedCatalog {
    catalog: Arc<SkillCatalog>,
    loaded_at: Instant,
}

/// Discovers and caches skills from the filesystem.
pub struct SkillLoader {
    cache: RwLock<HashMap<(PathBuf, PathBuf), CachedCatalog>>,
    ttl: Duration,
}

impl SkillLoader {
    /// Creates a new `SkillLoader` with the given cache TTL.
    pub fn new(ttl: Duration) -> Self {
        Self {
            cache: RwLock::new(HashMap::new()),
            ttl,
        }
    }

    /// Invalidates all cached catalogs, forcing a rescan on the next `load()` call.
    pub fn invalidate_cache(&self) {
        if let Ok(mut cache) = self.cache.write() {
            cache.clear();
        }
    }

    /// Returns a skill catalog for the given user home and workspace root.
    ///
    /// Results are cached per `(user_home, workspace_root)` key. The cache
    /// entry is considered valid for the configured TTL; once expired the
    /// directories are re-scanned on the next call.
    pub async fn load(
        &self,
        user_home: &Path,
        workspace_root: &Path,
    ) -> Result<Arc<SkillCatalog>, SkillError> {
        let key = (user_home.to_path_buf(), workspace_root.to_path_buf());

        // Check cache
        {
            let cache = self.cache.read().map_err(|_| {
                SkillError::IoError(std::io::Error::other("skill cache lock poisoned"))
            })?;
            if let Some(cached) = cache.get(&key)
                && cached.loaded_at.elapsed() < self.ttl
            {
                return Ok(Arc::clone(&cached.catalog));
            }
        }

        // Cache miss — rescan
        let catalog = Arc::new(self.scan(user_home, workspace_root).await?);

        // Update cache
        {
            let mut cache = self.cache.write().map_err(|_| {
                SkillError::IoError(std::io::Error::other("skill cache lock poisoned"))
            })?;
            cache.insert(
                key,
                CachedCatalog {
                    catalog: Arc::clone(&catalog),
                    loaded_at: Instant::now(),
                },
            );
        }

        Ok(catalog)
    }

    async fn scan(
        &self,
        user_home: &Path,
        workspace_root: &Path,
    ) -> Result<SkillCatalog, SkillError> {
        let mut skills = HashMap::new();

        // Scan user-level directories first (lower priority)
        for dir in [
            user_home.join(".sober/skills"),
            user_home.join(".agents/skills"),
        ] {
            if dir.is_dir() {
                self.scan_directory(&dir, SkillSource::User, &mut skills)
                    .await;
            }
        }

        // Scan workspace-level directories (higher priority — overwrites user)
        for dir in [
            workspace_root.join(".sober/skills"),
            workspace_root.join(".agents/skills"),
        ] {
            if dir.is_dir() {
                self.scan_directory(&dir, SkillSource::Workspace, &mut skills)
                    .await;
            }
        }

        debug!("loaded {} skills", skills.len());
        Ok(SkillCatalog::new(skills))
    }

    async fn scan_directory(
        &self,
        dir: &Path,
        source: SkillSource,
        skills: &mut HashMap<String, SkillEntry>,
    ) {
        let mut dirs_scanned = 0;

        let Ok(mut entries) = fs::read_dir(dir).await else {
            warn!(?dir, "failed to read skill directory");
            return;
        };

        while let Ok(Some(entry)) = entries.next_entry().await {
            if dirs_scanned >= MAX_SCAN_DIRS {
                warn!(?dir, "hit max directory scan limit");
                break;
            }

            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str.starts_with('.') || name_str == "node_modules" {
                continue;
            }

            dirs_scanned += 1;

            let skill_md = path.join("SKILL.md");
            if !skill_md.exists() {
                continue;
            }

            match self.load_skill(&skill_md, &path, source).await {
                Ok(entry_val) => {
                    let skill_name = entry_val.frontmatter.name.clone();
                    if skills.contains_key(&skill_name) && source == SkillSource::User {
                        debug!(
                            skill_name,
                            "skill already loaded from higher-priority scope"
                        );
                        continue;
                    }
                    if skills.contains_key(&skill_name) {
                        warn!(
                            skill_name,
                            "skill name collision — overriding with {:?}", source
                        );
                    }
                    skills.insert(skill_name, entry_val);
                }
                Err(e) => {
                    warn!(?skill_md, error = %e, "skipping skill");
                }
            }
        }
    }

    async fn load_skill(
        &self,
        skill_md: &Path,
        base_dir: &Path,
        source: SkillSource,
    ) -> Result<SkillEntry, SkillError> {
        let content = fs::read_to_string(skill_md).await?;
        let (frontmatter, _body) =
            parse_skill_frontmatter(&content).map_err(|e| SkillError::FrontmatterParseFailed {
                path: skill_md.display().to_string(),
                reason: e.to_string(),
            })?;

        let warnings = validate_skill_name(&frontmatter.name);
        for w in &warnings {
            warn!(skill = %frontmatter.name, path = ?skill_md, "name validation: {w}");
        }

        if let Some(dir_name) = base_dir.file_name().and_then(|n| n.to_str())
            && dir_name != frontmatter.name
        {
            warn!(
                skill = %frontmatter.name,
                dir = dir_name,
                "skill name does not match directory name"
            );
        }

        Ok(SkillEntry {
            frontmatter,
            path: skill_md.to_path_buf(),
            base_dir: base_dir.to_path_buf(),
            source,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn setup_user_home() -> (tempfile::TempDir, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().to_path_buf();
        let skills_dir = home.join(".sober/skills");
        let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");

        std::fs::create_dir_all(&skills_dir).unwrap();
        for entry in std::fs::read_dir(&fixtures).unwrap() {
            let entry = entry.unwrap();
            if entry.path().is_dir() {
                let dest = skills_dir.join(entry.file_name());
                copy_dir_recursive(&entry.path(), &dest);
            }
        }

        (tmp, home)
    }

    fn copy_dir_recursive(src: &Path, dst: &Path) {
        std::fs::create_dir_all(dst).unwrap();
        for entry in std::fs::read_dir(src).unwrap() {
            let entry = entry.unwrap();
            let dest = dst.join(entry.file_name());
            if entry.path().is_dir() {
                copy_dir_recursive(&entry.path(), &dest);
            } else {
                std::fs::copy(entry.path(), dest).unwrap();
            }
        }
    }

    #[tokio::test]
    async fn loads_valid_skills_from_directory() {
        let (_tmp, user_home) = setup_user_home();
        let loader = SkillLoader::new(Duration::from_secs(300));
        let catalog = loader
            .load(&user_home, &PathBuf::from("/nonexistent"))
            .await
            .unwrap();

        assert!(catalog.get("valid-skill").is_some());
        assert!(catalog.get("with-scripts").is_some());
    }

    #[tokio::test]
    async fn skips_malformed_frontmatter() {
        let (_tmp, user_home) = setup_user_home();
        let loader = SkillLoader::new(Duration::from_secs(300));
        let catalog = loader
            .load(&user_home, &PathBuf::from("/nonexistent"))
            .await
            .unwrap();

        assert!(catalog.get("malformed-yaml").is_none());
    }

    #[tokio::test]
    async fn skips_missing_description() {
        let (_tmp, user_home) = setup_user_home();
        let loader = SkillLoader::new(Duration::from_secs(300));
        let catalog = loader
            .load(&user_home, &PathBuf::from("/nonexistent"))
            .await
            .unwrap();

        assert!(catalog.get("missing-description").is_none());
    }

    #[tokio::test]
    async fn warns_on_bad_name() {
        let (_tmp, user_home) = setup_user_home();
        let loader = SkillLoader::new(Duration::from_secs(300));
        let catalog = loader
            .load(&user_home, &PathBuf::from("/nonexistent"))
            .await
            .unwrap();

        // Loaded with the frontmatter name (Bad--Name), warnings logged
        assert!(catalog.get("Bad--Name").is_some());
    }

    #[tokio::test]
    async fn caches_results_within_ttl() {
        let (_tmp, user_home) = setup_user_home();
        let loader = SkillLoader::new(Duration::from_secs(300));
        let empty = PathBuf::from("/nonexistent");

        let cat1 = loader.load(&user_home, &empty).await.unwrap();
        let cat2 = loader.load(&user_home, &empty).await.unwrap();

        assert!(Arc::ptr_eq(&cat1, &cat2));
    }

    #[tokio::test]
    async fn workspace_overrides_user_on_collision() {
        let tmp = tempfile::tempdir().unwrap();
        let user_dir = tmp.path().join("user-skills");
        let ws_dir = tmp.path().join("ws-skills");

        let user_skill = user_dir.join(".sober/skills/my-skill");
        std::fs::create_dir_all(&user_skill).unwrap();
        std::fs::write(
            user_skill.join("SKILL.md"),
            "---\nname: my-skill\ndescription: User version.\n---\nUser body.",
        )
        .unwrap();

        let ws_skill = ws_dir.join(".sober/skills/my-skill");
        std::fs::create_dir_all(&ws_skill).unwrap();
        std::fs::write(
            ws_skill.join("SKILL.md"),
            "---\nname: my-skill\ndescription: Workspace version.\n---\nWS body.",
        )
        .unwrap();

        let loader = SkillLoader::new(Duration::from_secs(0));
        let catalog = loader.load(&user_dir, &ws_dir).await.unwrap();

        let entry = catalog.get("my-skill").unwrap();
        assert_eq!(entry.source, SkillSource::Workspace);
        assert_eq!(entry.frontmatter.description, "Workspace version.");
    }
}
