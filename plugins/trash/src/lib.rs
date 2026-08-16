use lazy_static::lazy_static;
use lla_plugin_sdk::{interface::proto, ActionArguments, DecoratedEntryExt, Plugin};
use lla_plugin_utils::{
    decode_decorated_entry, map_decorated_entry, run_cli_action, trash::TrashStore, ActionRegistry,
    DecoratedEntry,
};
use parking_lot::RwLock;
use std::path::Path;

lazy_static! {
    static ref ACTIONS: RwLock<ActionRegistry> = RwLock::new({
        let mut actions = ActionRegistry::new();
        lla_plugin_utils::define_action!(
            actions,
            "put",
            "put <path>...",
            "Move one or more files or directories into recoverable trash",
            ["lla plugin run trash put -- ./draft.txt ./old-folder"],
            TrashPlugin::put_action
        );
        lla_plugin_utils::define_action!(
            actions,
            "list",
            "list",
            "List recoverable trash records and their original paths",
            ["lla plugin run trash list"],
            |_| TrashPlugin::list_action()
        );
        lla_plugin_utils::define_action!(
            actions,
            "restore",
            "restore <id>",
            "Restore a trashed item without overwriting an existing path",
            ["lla plugin run trash restore -- 20260815T120000.000Z-123-0"],
            TrashPlugin::restore_action
        );
        lla_plugin_utils::define_action!(
            actions,
            "empty",
            "empty [older-than-days] --yes",
            "Permanently delete old trash records after explicit confirmation",
            ["lla plugin run trash empty -- 30 -- --yes"],
            TrashPlugin::empty_action
        );
        lla_plugin_utils::define_action!(
            actions,
            "help",
            "help",
            "Show recoverable trash usage",
            ["lla plugin run trash help"],
            |_| TrashPlugin::help_action()
        );
        actions
    });
}

pub struct TrashPlugin;

impl TrashPlugin {
    fn decorate(mut entry: DecoratedEntry) -> DecoratedEntry {
        let store = TrashStore::for_plugin_data();
        let in_trash = entry.path.starts_with(store.root());
        entry
            .custom_fields
            .insert("trashable".to_string(), (!in_trash).to_string());
        entry.custom_fields.insert(
            "trash_state".to_string(),
            if in_trash { "trashed" } else { "available" }.to_string(),
        );
        entry
    }

    fn format(entry: &DecoratedEntry, format: &str) -> Option<String> {
        let state = entry.custom_fields.get("trash_state")?;
        match format {
            "default" => Some(format!("[trash: {state}]")),
            "long" => Some(format!(
                "Trash state: {state}\nRecoverable deletion: {}",
                entry.custom_fields.get("trashable")?
            )),
            _ => None,
        }
    }

    fn put_action(args: &[String]) -> Result<(), String> {
        if args.is_empty() {
            return Err("Usage: put <path>...".to_string());
        }
        let store = TrashStore::for_plugin_data();
        let mut failures = Vec::new();
        for value in args {
            if value.starts_with('-') {
                failures.push(format!("Unknown option: {value}"));
                continue;
            }
            match store.put(Path::new(value)) {
                Ok(record) => println!(
                    "Trashed {}\n  id: {}\n  restore: lla plugin run trash restore -- {}",
                    record.original_path.display(),
                    record.id,
                    record.id
                ),
                Err(error) => failures.push(error),
            }
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(failures.join("\n"))
        }
    }

    fn list_action() -> Result<(), String> {
        let records = TrashStore::for_plugin_data().list()?;
        if records.is_empty() {
            println!("Trash is empty.");
            return Ok(());
        }
        for record in records {
            println!(
                "{}  {}  {} bytes  {}",
                record.id,
                record.deleted_at,
                record.size,
                record.original_path.display()
            );
        }
        Ok(())
    }

    fn restore_action(args: &[String]) -> Result<(), String> {
        let id = args
            .first()
            .ok_or_else(|| "Usage: restore <id>".to_string())?;
        let restored = TrashStore::for_plugin_data().restore(id)?;
        println!("Restored to {}", restored.display());
        Ok(())
    }

    fn empty_action(args: &[String]) -> Result<(), String> {
        if !args.iter().any(|arg| arg == "--yes") {
            return Err(
                "Permanent deletion requires --yes. Example: lla plugin run trash empty -- 30 -- --yes"
                    .to_string(),
            );
        }
        let days = args
            .iter()
            .find(|arg| !arg.starts_with('-'))
            .map(|value| value.parse::<u64>())
            .transpose()
            .map_err(|_| "older-than-days must be a non-negative integer".to_string())?
            .unwrap_or(30);
        let removed = TrashStore::for_plugin_data().empty_older_than(days)?;
        println!("Permanently removed {removed} trash item(s) at least {days} day(s) old.");
        Ok(())
    }

    fn help_action() -> Result<(), String> {
        println!(
            "trash\n\n  put <path>...              Recoverably delete files or directories\n  list                       List trash ids and original paths\n  restore <id>               Restore without overwriting conflicts\n  empty [days] --yes         Permanently remove old trash\n  help                       Show this help\n\nTrash data lives in the lla plugin data directory and works consistently across supported platforms."
        );
        Ok(())
    }
}

impl Plugin for TrashPlugin {
    fn decorate_entry(&mut self, entry: proto::DecoratedEntry) -> proto::DecoratedEntry {
        promote_v3_fields(map_decorated_entry(entry, Self::decorate))
    }

    fn decorate_batch(
        &mut self,
        entries: Vec<proto::DecoratedEntry>,
        _format: &str,
    ) -> Vec<proto::DecoratedEntry> {
        entries
            .into_iter()
            .map(|entry| promote_v3_fields(map_decorated_entry(entry, Self::decorate)))
            .collect()
    }

    fn format_field(&mut self, entry: proto::DecoratedEntry, format: String) -> Option<String> {
        decode_decorated_entry(entry)
            .ok()
            .and_then(|entry| Self::format(&entry, &format))
    }

    fn run_action(&mut self, action: String, arguments: ActionArguments) -> proto::ActionResponse {
        run_cli_action(
            &action,
            arguments,
            include_str!("../plugin.toml"),
            |arguments| ACTIONS.read().handle(&action, arguments),
        )
    }

    fn registered_actions(&mut self) -> Vec<proto::ActionInfo> {
        lla_plugin_utils::manifest_action_infos(include_str!("../plugin.toml"))
    }
}

fn promote_v3_fields(mut entry: proto::DecoratedEntry) -> proto::DecoratedEntry {
    entry.promote_boolean_field("trashable");
    entry.promote_string_field("trash_state");
    entry
}

lla_plugin_sdk::export_plugin!(TrashPlugin);

impl Default for TrashPlugin {
    fn default() -> Self {
        Self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permanent_emptying_requires_explicit_confirmation() {
        let error = TrashPlugin::empty_action(&[]).unwrap_err();
        assert!(error.contains("requires --yes"));
    }
}
