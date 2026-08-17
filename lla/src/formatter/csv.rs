use crate::error::Result;
use crate::plugin::PluginManager;
use lla_plugin_interface::proto::DecoratedEntry;

use super::serializable::{find_git_root, get_git_status_map, to_serializable};

pub fn write_csv_stream<I>(
    entries: I,
    _plugin_manager: &mut PluginManager,
    include_git_status: bool,
) -> Result<()>
where
    I: IntoIterator<Item = DecoratedEntry>,
{
    let stdout = std::io::stdout();
    let handle = stdout.lock();
    let mut wtr = csv::Writer::from_writer(handle);

    wtr.write_record([
        "path",
        "name",
        "extension",
        "file_type",
        "size_bytes",
        "modified",
        "created",
        "accessed",
        "mode_octal",
        "owner_user",
        "owner_group",
        "inode",
        "hard_links",
        "allocated_size_bytes",
        "xattrs",
        "has_acl",
        "security_context",
        "mount_point",
        "mount_source",
        "filesystem",
        "symlink_target",
        "is_hidden",
        "git_status",
    ])?;

    let mut git_status_map = None;
    let mut git_root = None;

    for entry in entries {
        let git_status = if include_git_status {
            if git_root.is_none() {
                if let Some(parent) = std::path::Path::new(&entry.path).parent() {
                    if let Some(root) = find_git_root(parent) {
                        git_root = Some(root.clone());
                        git_status_map = Some(get_git_status_map(&root));
                    }
                }
            }
            if let Some(root) = &git_root {
                let full = std::path::Path::new(&entry.path);
                let rel = full.strip_prefix(root).unwrap_or(full);
                let rel_str = rel.to_string_lossy().to_string();
                git_status_map
                    .as_ref()
                    .and_then(|m| m.get(&rel_str))
                    .cloned()
            } else {
                None
            }
        } else {
            None
        };

        let serial = to_serializable(&entry, git_status);

        wtr.write_record(&[
            serial.path,
            serial.name,
            serial.extension.unwrap_or_default(),
            serial.file_type,
            serial.size_bytes.to_string(),
            serial.modified,
            serial.created.unwrap_or_default(),
            serial.accessed.unwrap_or_default(),
            serial.mode_octal,
            serial.owner_user.unwrap_or_default(),
            serial.owner_group.unwrap_or_default(),
            serial.inode.map(|v| v.to_string()).unwrap_or_default(),
            serial.hard_links.map(|v| v.to_string()).unwrap_or_default(),
            serial
                .allocated_size_bytes
                .map(|v| v.to_string())
                .unwrap_or_default(),
            serde_json::to_string(&serial.xattrs).unwrap_or_else(|_| "{}".to_string()),
            serial.has_acl.to_string(),
            serial.security_context.unwrap_or_default(),
            serial.mount_point.unwrap_or_default(),
            serial.mount_source.unwrap_or_default(),
            serial.filesystem.unwrap_or_default(),
            serial.symlink_target.unwrap_or_default(),
            serial.is_hidden.to_string(),
            serial.git_status.unwrap_or_default(),
        ])?;
    }

    wtr.flush()?;
    Ok(())
}
