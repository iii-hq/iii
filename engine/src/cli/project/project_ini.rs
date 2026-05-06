// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

use std::path::Path;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectIni {
    pub project_id: Option<String>,
    pub project_name: Option<String>,
    pub source: Option<String>,
    pub device_id: Option<String>,
}

impl ProjectIni {
    pub fn write(&self, project_root: &Path) -> std::io::Result<()> {
        let dir = project_root.join(".iii");
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("project.ini");

        let mut out = String::new();
        if let Some(v) = &self.project_id {
            out.push_str(&format!("project_id={}\n", v));
        }
        if let Some(v) = &self.project_name {
            out.push_str(&format!("project_name={}\n", v));
        }
        if let Some(v) = &self.source {
            out.push_str(&format!("source={}\n", v));
        }
        if let Some(v) = &self.device_id {
            out.push_str(&format!("device_id={}\n", v));
        }
        std::fs::write(path, out)
    }

    pub fn read(project_root: &Path) -> std::io::Result<Self> {
        let path = project_root.join(".iii").join("project.ini");
        let contents = std::fs::read_to_string(path)?;
        let mut out = Self::default();
        for line in contents.lines() {
            let line = line.trim();
            if let Some(v) = line.strip_prefix("project_id=") {
                out.project_id = Some(v.trim().to_string());
            } else if let Some(v) = line.strip_prefix("project_name=") {
                out.project_name = Some(v.trim().to_string());
            } else if let Some(v) = line.strip_prefix("source=") {
                out.source = Some(v.trim().to_string());
            } else if let Some(v) = line.strip_prefix("device_id=") {
                out.device_id = Some(v.trim().to_string());
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn write_and_read_round_trip() {
        let dir = tempdir().unwrap();
        let ini = ProjectIni {
            project_id: Some("proj-123".to_string()),
            project_name: Some("myapp".to_string()),
            source: Some("init".to_string()),
            device_id: Some("device-abc".to_string()),
        };
        ini.write(dir.path()).unwrap();
        let read = ProjectIni::read(dir.path()).unwrap();
        assert_eq!(read, ini);
    }

    #[test]
    fn write_creates_dot_iii_dir() {
        let dir = tempdir().unwrap();
        let ini = ProjectIni {
            project_id: Some("p".to_string()),
            ..Default::default()
        };
        ini.write(dir.path()).unwrap();
        assert!(dir.path().join(".iii").join("project.ini").exists());
    }

    #[test]
    fn write_skips_none_fields() {
        let dir = tempdir().unwrap();
        let ini = ProjectIni {
            project_id: Some("p".to_string()),
            ..Default::default()
        };
        ini.write(dir.path()).unwrap();
        let contents =
            std::fs::read_to_string(dir.path().join(".iii").join("project.ini")).unwrap();
        assert!(contents.contains("project_id=p"));
        assert!(!contents.contains("project_name="));
        assert!(!contents.contains("device_id="));
    }
}
