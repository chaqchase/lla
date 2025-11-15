#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ColumnKey {
    Permissions,
    Size,
    Modified,
    Created,
    Accessed,
    User,
    Group,
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
            "size" => ColumnKey::Size,
            "modified" | "modified_at" | "date" => ColumnKey::Modified,
            "created" => ColumnKey::Created,
            "accessed" | "access" => ColumnKey::Accessed,
            "user" | "owner" => ColumnKey::User,
            "group" => ColumnKey::Group,
            "name" => ColumnKey::Name,
            "path" => ColumnKey::Path,
            "plugins" | "plugin" => ColumnKey::Plugins,
            _ => ColumnKey::CustomField(trimmed.to_string()),
        }
    }

    pub fn align_right(&self) -> bool {
        matches!(self, ColumnKey::Size)
    }

    pub fn header_label(&self) -> String {
        match self {
            ColumnKey::Permissions => "Permissions".to_string(),
            ColumnKey::Size => "Size".to_string(),
            ColumnKey::Modified => "Modified".to_string(),
            ColumnKey::Created => "Created".to_string(),
            ColumnKey::Accessed => "Accessed".to_string(),
            ColumnKey::User => "User".to_string(),
            ColumnKey::Group => "Group".to_string(),
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
