#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ColumnKey {
    Permissions,
    Inode,
    HardLinks,
    Size,
    AllocatedSize,
    Modified,
    Created,
    Accessed,
    User,
    Group,
    Xattrs,
    Context,
    Mount,
    Name,
    Path,
    Plugins,
    CustomField(String),
}

pub fn parse_columns(values: &[String]) -> Vec<ColumnKey> {
    let mut columns: Vec<ColumnKey> = values
        .iter()
        .map(|value| ColumnKey::from_config(value))
        .collect();

    if columns.is_empty() {
        columns.push(ColumnKey::Name);
    }
    columns
}

impl ColumnKey {
    pub fn from_config(raw: &str) -> ColumnKey {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return ColumnKey::Name;
        }
        if let Some(field) = trimmed.strip_prefix("field:") {
            return ColumnKey::CustomField(field.trim().to_string());
        }
        match trimmed.to_lowercase().as_str() {
            "permissions" | "perms" => ColumnKey::Permissions,
            "inode" | "ino" => ColumnKey::Inode,
            "links" | "hard_links" | "hard-links" => ColumnKey::HardLinks,
            "size" => ColumnKey::Size,
            "allocated" | "allocated_size" | "allocated-size" | "blocks" | "blocksize" => {
                ColumnKey::AllocatedSize
            }
            "modified" | "modified_at" | "date" => ColumnKey::Modified,
            "created" => ColumnKey::Created,
            "accessed" | "access" => ColumnKey::Accessed,
            "user" | "owner" => ColumnKey::User,
            "group" => ColumnKey::Group,
            "xattrs" | "extended" => ColumnKey::Xattrs,
            "context" | "security_context" | "security-context" | "acl" => ColumnKey::Context,
            "mount" | "mounts" => ColumnKey::Mount,
            "name" => ColumnKey::Name,
            "path" => ColumnKey::Path,
            "plugins" | "plugin" => ColumnKey::Plugins,
            _ => ColumnKey::CustomField(trimmed.to_string()),
        }
    }

    pub fn align_right(&self) -> bool {
        matches!(
            self,
            ColumnKey::Inode | ColumnKey::HardLinks | ColumnKey::Size | ColumnKey::AllocatedSize
        )
    }

    pub fn header_label(&self) -> String {
        match self {
            ColumnKey::Permissions => "Permissions".to_string(),
            ColumnKey::Inode => "Inode".to_string(),
            ColumnKey::HardLinks => "Links".to_string(),
            ColumnKey::Size => "Size".to_string(),
            ColumnKey::AllocatedSize => "Allocated".to_string(),
            ColumnKey::Modified => "Modified".to_string(),
            ColumnKey::Created => "Created".to_string(),
            ColumnKey::Accessed => "Accessed".to_string(),
            ColumnKey::User => "User".to_string(),
            ColumnKey::Group => "Group".to_string(),
            ColumnKey::Xattrs => "Xattrs".to_string(),
            ColumnKey::Context => "Context".to_string(),
            ColumnKey::Mount => "Mount".to_string(),
            ColumnKey::Name => "Name".to_string(),
            ColumnKey::Path => "Path".to_string(),
            ColumnKey::Plugins => "Plugins".to_string(),
            ColumnKey::CustomField(field) => field.clone(),
        }
    }

    pub fn is_group(&self) -> bool {
        matches!(self, ColumnKey::Group)
    }

    pub fn is_plugins(&self) -> bool {
        matches!(self, ColumnKey::Plugins)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_rich_metadata_columns_and_aliases() {
        assert_eq!(ColumnKey::from_config("inode"), ColumnKey::Inode);
        assert_eq!(ColumnKey::from_config("hard-links"), ColumnKey::HardLinks);
        assert_eq!(ColumnKey::from_config("blocks"), ColumnKey::AllocatedSize);
        assert_eq!(ColumnKey::from_config("extended"), ColumnKey::Xattrs);
        assert_eq!(
            ColumnKey::from_config("security-context"),
            ColumnKey::Context
        );
        assert_eq!(ColumnKey::from_config("mounts"), ColumnKey::Mount);
    }
}
