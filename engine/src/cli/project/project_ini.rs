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
        write_field(&mut out, "project_id", self.project_id.as_deref())?;
        write_field(&mut out, "project_name", self.project_name.as_deref())?;
        write_field(&mut out, "source", self.source.as_deref())?;
        write_field(&mut out, "device_id", self.device_id.as_deref())?;
        std::fs::write(path, out)
    }

}

/// Append a `key=value\n` line to the buffer, rejecting values that contain
/// newline characters. The flat ini format we use parses one field per line,
/// so embedded `\n` or `\r` would silently corrupt subsequent fields.
fn write_field(out: &mut String, key: &str, value: Option<&str>) -> std::io::Result<()> {
    let Some(v) = value else { return Ok(()) };
    if v.contains('\n') || v.contains('\r') {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("{key} value must not contain newline characters"),
        ));
    }
    out.push_str(key);
    out.push('=');
    out.push_str(v);
    out.push('\n');
    Ok(())
}

impl ProjectIni {
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

    #[test]
    fn write_rejects_newline_in_value() {
        let dir = tempdir().unwrap();
        let ini = ProjectIni {
            project_name: Some("evil\nproject_id=spoofed".to_string()),
            ..Default::default()
        };
        let err = ini.write(dir.path()).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    }

    #[test]
    fn write_rejects_carriage_return_in_value() {
        let dir = tempdir().unwrap();
        let ini = ProjectIni {
            device_id: Some("with\rcr".to_string()),
            ..Default::default()
        };
        let err = ini.write(dir.path()).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    }
}
