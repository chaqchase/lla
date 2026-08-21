use crate::DecoratedEntry;
use std::collections::HashMap;

type FieldFormatter = dyn Fn(&DecoratedEntry) -> Option<String>;

pub trait EntryFormatter {
    fn format_field(&self, entry: &DecoratedEntry, format: &str) -> Option<String>;
}

pub struct FieldFormatterBuilder {
    formatters: HashMap<String, Box<FieldFormatter>>,
}

impl FieldFormatterBuilder {
    pub fn new() -> Self {
        Self {
            formatters: HashMap::new(),
        }
    }

    pub fn add_formatter<F>(mut self, format: &str, formatter: F) -> Self
    where
        F: Fn(&DecoratedEntry) -> Option<String> + 'static,
    {
        self.formatters
            .insert(format.to_string(), Box::new(formatter));
        self
    }

    pub fn build(self) -> CustomFieldFormatter {
        CustomFieldFormatter {
            formatters: self.formatters,
        }
    }
}

pub struct CustomFieldFormatter {
    formatters: HashMap<String, Box<FieldFormatter>>,
}

impl EntryFormatter for CustomFieldFormatter {
    fn format_field(&self, entry: &DecoratedEntry, format: &str) -> Option<String> {
        self.formatters.get(format).and_then(|f| f(entry))
    }
}

impl Default for FieldFormatterBuilder {
    fn default() -> Self {
        Self::new()
    }
}

pub fn format_permissions(permissions: u32) -> String {
    let mut result = String::with_capacity(10);

    result.push(if permissions & 0o040000 != 0 {
        'd'
    } else {
        '-'
    });

    result.push(if permissions & 0o400 != 0 { 'r' } else { '-' });
    result.push(if permissions & 0o200 != 0 { 'w' } else { '-' });
    result.push(if permissions & 0o100 != 0 { 'x' } else { '-' });

    result.push(if permissions & 0o040 != 0 { 'r' } else { '-' });
    result.push(if permissions & 0o020 != 0 { 'w' } else { '-' });
    result.push(if permissions & 0o010 != 0 { 'x' } else { '-' });

    result.push(if permissions & 0o004 != 0 { 'r' } else { '-' });
    result.push(if permissions & 0o002 != 0 { 'w' } else { '-' });
    result.push(if permissions & 0o001 != 0 { 'x' } else { '-' });

    result
}

pub fn format_file_type(entry: &DecoratedEntry) -> String {
    if entry.metadata.is_dir {
        "Directory".to_string()
    } else if entry.metadata.is_symlink {
        "Symlink".to_string()
    } else if entry.metadata.is_file {
        match entry.path.extension() {
            Some(ext) => ext.to_string_lossy().to_uppercase(),
            None => "File".to_string(),
        }
    } else {
        "Unknown".to_string()
    }
}

pub fn format_ownership(uid: u32, gid: u32) -> String {
    #[cfg(unix)]
    {
        use users::{get_group_by_gid, get_user_by_uid};

        let user = get_user_by_uid(uid)
            .map(|u| u.name().to_string_lossy().into_owned())
            .unwrap_or_else(|| uid.to_string());

        let group = get_group_by_gid(gid)
            .map(|g| g.name().to_string_lossy().into_owned())
            .unwrap_or_else(|| gid.to_string());

        format!("{}:{}", user, group)
    }

    #[cfg(windows)]
    {
        let _ = (uid, gid);
        "-:-".to_string()
    }
}
