use lazy_static::lazy_static;
use lla_plugin_sdk::{interface::proto, value, ActionArguments, DecoratedEntryExt, Plugin};
use lla_plugin_utils::DecoratedEntry;
use lla_plugin_utils::{
    config::PluginConfig,
    decode_decorated_entry, run_cli_action,
    ui::{
        components::{BoxComponent, BoxStyle, HelpFormatter, KeyValue, List, Spinner},
        TextBlock,
    },
    ActionRegistry, BasePlugin, ConfigurablePlugin,
};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs::File,
    io::Read,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

lazy_static! {
    static ref SPINNER: RwLock<Spinner> = RwLock::new(Spinner::new());
    static ref ACTION_REGISTRY: RwLock<ActionRegistry> = RwLock::new({
        let mut registry = ActionRegistry::new();

        lla_plugin_utils::define_action!(
            registry,
            "clear-cache",
            "clear-cache",
            "Clear the duplicate file detection cache",
            ["lla plugin run duplicate_file_detector clear-cache"],
            |_| {
                let spinner = SPINNER.write();
                spinner.set_status("Clearing cache...".to_string());
                spinner.finish();
                println!(
                    "{}",
                    BoxComponent::new(
                        TextBlock::new("Cache cleared successfully")
                            .color("bright_green")
                            .build()
                    )
                    .style(BoxStyle::Minimal)
                    .padding(1)
                    .render()
                );
                Ok(())
            }
        );

        lla_plugin_utils::define_action!(
            registry,
            "help",
            "help",
            "Show help information",
            ["lla plugin run duplicate_file_detector help"],
            |_| {
                let mut help = HelpFormatter::new("Duplicate File Detector Plugin".to_string());
                help.add_section("Description".to_string()).add_command(
                    "".to_string(),
                    "Detects duplicate files by comparing their content hashes.".to_string(),
                    vec![],
                );

                help.add_section("Actions".to_string())
                    .add_command(
                        "clear-cache".to_string(),
                        "Clear the duplicate file detection cache".to_string(),
                        vec!["lla plugin run duplicate_file_detector clear-cache".to_string()],
                    )
                    .add_command(
                        "help".to_string(),
                        "Show this help information".to_string(),
                        vec!["lla plugin run duplicate_file_detector help".to_string()],
                    );

                help.add_section("Formats".to_string())
                    .add_command(
                        "default".to_string(),
                        "Show basic duplicate information".to_string(),
                        vec![],
                    )
                    .add_command(
                        "long".to_string(),
                        "Show detailed duplicate information including paths".to_string(),
                        vec![],
                    );

                println!(
                    "{}",
                    BoxComponent::new(help.render(&DuplicateConfig::default().colors))
                        .style(BoxStyle::Minimal)
                        .padding(2)
                        .render()
                );
                Ok(())
            }
        );

        registry
    });
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DuplicateConfig {
    #[serde(default = "default_colors")]
    colors: HashMap<String, String>,
}

fn default_colors() -> HashMap<String, String> {
    let mut colors = HashMap::new();
    colors.insert("duplicate".to_string(), "bright_red".to_string());
    colors.insert("has_duplicates".to_string(), "bright_yellow".to_string());
    colors.insert("path".to_string(), "bright_cyan".to_string());
    colors.insert("success".to_string(), "bright_green".to_string());
    colors.insert("info".to_string(), "bright_blue".to_string());
    colors.insert("name".to_string(), "bright_yellow".to_string());
    colors
}

impl Default for DuplicateConfig {
    fn default() -> Self {
        Self {
            colors: default_colors(),
        }
    }
}

impl PluginConfig for DuplicateConfig {}

pub struct DuplicateFileDetectorPlugin {
    base: BasePlugin<DuplicateConfig>,
    hash_cache: lla_plugin_utils::PersistentCache<String>,
}

impl DuplicateFileDetectorPlugin {
    pub fn new() -> Self {
        let plugin_name = env!("CARGO_PKG_NAME");
        let plugin = Self {
            base: BasePlugin::with_name(plugin_name),
            hash_cache: lla_plugin_utils::PersistentCache::for_plugin(
                plugin_name,
                "hash-cache.toml",
                1,
                50_000,
            ),
        };
        if let Err(e) = plugin.base.save_config() {
            eprintln!("[DuplicateFileDetectorPlugin] Failed to save config: {}", e);
        }
        plugin
    }

    fn get_file_hash(path: &Path) -> Option<String> {
        let mut file = File::open(path).ok()?;
        let mut hasher = Sha256::new();
        let mut buffer = [0u8; 8192];

        loop {
            match file.read(&mut buffer) {
                Ok(0) => break,
                Ok(n) => hasher.update(&buffer[..n]),
                Err(_) => return None,
            }
        }
        Some(format!("{:x}", hasher.finalize()))
    }

    fn hash_for(&mut self, path: &Path) -> Option<String> {
        let key = lla_plugin_utils::canonical_cache_key(path);
        let fingerprint = lla_plugin_utils::file_fingerprint(path).ok()?;
        if let Some(hash) = self.hash_cache.get(&key, &fingerprint) {
            return Some(hash);
        }
        let hash = Self::get_file_hash(path)?;
        self.hash_cache.insert(key, fingerprint, hash.clone());
        Some(hash)
    }

    fn process_batch(
        &mut self,
        mut entries: Vec<proto::DecoratedEntry>,
    ) -> Vec<proto::DecoratedEntry> {
        let spinner = SPINNER.write();
        spinner.set_status("Checking for duplicates...".to_string());

        let mut by_size = HashMap::<u64, Vec<usize>>::new();
        for (index, entry) in entries.iter().enumerate() {
            if let Some(metadata) = entry.metadata.as_ref().filter(|metadata| metadata.is_file) {
                by_size.entry(metadata.size).or_default().push(index);
            }
        }

        let mut by_hash = HashMap::<String, Vec<usize>>::new();
        for candidates in by_size.into_values().filter(|indices| indices.len() > 1) {
            for index in candidates {
                if let Some(hash) = self.hash_for(Path::new(&entries[index].path)) {
                    by_hash.entry(hash).or_default().push(index);
                }
            }
        }

        mark_duplicate_groups(&mut entries, by_hash);

        let _ = self.hash_cache.persist();
        spinner.finish();
        entries
    }

    fn format_duplicate_info(&self, entry: &DecoratedEntry, format: &str) -> Option<String> {
        let colors = &self.base.config().colors;
        let mut list = List::new().style(BoxStyle::Minimal).key_width(15);

        if entry.custom_fields.contains_key("has_duplicates") {
            match format {
                "long" => {
                    list.add_item(
                        KeyValue::new("Status", "HAS DUPLICATES")
                            .key_color(colors.get("info").unwrap_or(&"white".to_string()))
                            .value_color(
                                colors.get("has_duplicates").unwrap_or(&"white".to_string()),
                            )
                            .key_width(15)
                            .render(),
                    );

                    if let Some(paths) = entry.custom_fields.get("duplicate_paths") {
                        list.add_item(
                            KeyValue::new("Duplicate Copies", paths)
                                .key_color(colors.get("info").unwrap_or(&"white".to_string()))
                                .value_color(colors.get("path").unwrap_or(&"white".to_string()))
                                .key_width(15)
                                .render(),
                        );
                    }
                }
                "default" => {
                    if let Some(paths) = entry.custom_fields.get("duplicate_paths") {
                        list.add_item(
                            KeyValue::new("Status", format!("HAS DUPLICATES: {}", paths))
                                .key_color(colors.get("info").unwrap_or(&"white".to_string()))
                                .value_color(
                                    colors.get("has_duplicates").unwrap_or(&"white".to_string()),
                                )
                                .key_width(15)
                                .render(),
                        );
                    } else {
                        list.add_item(
                            KeyValue::new("Status", "HAS DUPLICATES")
                                .key_color(colors.get("info").unwrap_or(&"white".to_string()))
                                .value_color(
                                    colors.get("has_duplicates").unwrap_or(&"white".to_string()),
                                )
                                .key_width(15)
                                .render(),
                        );
                    }
                }
                _ => return None,
            }
        } else if entry.custom_fields.contains_key("is_duplicate") {
            match format {
                "long" => {
                    list.add_item(
                        KeyValue::new("Status", "DUPLICATE")
                            .key_color(colors.get("info").unwrap_or(&"white".to_string()))
                            .value_color(colors.get("duplicate").unwrap_or(&"white".to_string()))
                            .key_width(15)
                            .render(),
                    );

                    if let Some(original) = entry.custom_fields.get("original_path") {
                        list.add_item(
                            KeyValue::new("Original File", original)
                                .key_color(colors.get("info").unwrap_or(&"white".to_string()))
                                .value_color(colors.get("path").unwrap_or(&"white".to_string()))
                                .key_width(15)
                                .render(),
                        );
                    }
                }
                "default" => {
                    if let Some(original) = entry.custom_fields.get("original_path") {
                        list.add_item(
                            KeyValue::new("Status", format!("DUPLICATE of {}", original))
                                .key_color(colors.get("info").unwrap_or(&"white".to_string()))
                                .value_color(
                                    colors.get("duplicate").unwrap_or(&"white".to_string()),
                                )
                                .key_width(15)
                                .render(),
                        );
                    } else {
                        list.add_item(
                            KeyValue::new("Status", "DUPLICATE")
                                .key_color(colors.get("info").unwrap_or(&"white".to_string()))
                                .value_color(
                                    colors.get("duplicate").unwrap_or(&"white".to_string()),
                                )
                                .key_width(15)
                                .render(),
                        );
                    }
                }
                _ => return None,
            }
        } else {
            return None;
        }

        Some(format!("\n{}", list.render()))
    }
}

impl Plugin for DuplicateFileDetectorPlugin {
    fn decorate_entry(&mut self, entry: proto::DecoratedEntry) -> proto::DecoratedEntry {
        self.decorate_batch(vec![entry], "default")
            .into_iter()
            .next()
            .unwrap_or_default()
    }

    fn decorate_batch(
        &mut self,
        entries: Vec<proto::DecoratedEntry>,
        _format: &str,
    ) -> Vec<proto::DecoratedEntry> {
        self.process_batch(entries)
            .into_iter()
            .map(promote_duplicate_fields)
            .collect()
    }

    fn format_field(&mut self, entry: proto::DecoratedEntry, format: String) -> Option<String> {
        decode_decorated_entry(entry)
            .ok()
            .and_then(|entry| self.format_duplicate_info(&entry, &format))
    }

    fn run_action(&mut self, action: String, arguments: ActionArguments) -> proto::ActionResponse {
        if action == "clear-cache" {
            self.hash_cache.clear();
            let _ = self.hash_cache.persist();
        }
        run_cli_action(
            &action,
            arguments,
            include_str!("../plugin.toml"),
            |arguments| ACTION_REGISTRY.read().handle(&action, arguments),
        )
    }

    fn registered_actions(&mut self) -> Vec<proto::ActionInfo> {
        lla_plugin_utils::manifest_action_infos(include_str!("../plugin.toml"))
    }
}

fn modified_ns(path: &Path) -> u128 {
    path.metadata()
        .ok()
        .and_then(|metadata| metadata.modified().ok())
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_nanos())
        .unwrap_or_else(|| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        })
}

fn mark_duplicate_groups(
    entries: &mut [proto::DecoratedEntry],
    by_hash: HashMap<String, Vec<usize>>,
) {
    for duplicates in by_hash.into_values().filter(|indices| indices.len() > 1) {
        let original = duplicates
            .iter()
            .copied()
            .min_by_key(|index| modified_ns(Path::new(&entries[*index].path)))
            .unwrap_or(duplicates[0]);
        let original_path = entries[original].path.clone();
        let duplicate_paths = duplicates
            .iter()
            .copied()
            .filter(|index| *index != original)
            .map(|index| entries[index].path.clone())
            .collect::<Vec<_>>();

        for index in duplicates {
            if index == original {
                entries[index]
                    .custom_fields
                    .insert("has_duplicates".to_string(), "true".to_string());
                entries[index]
                    .custom_fields
                    .insert("duplicate_paths".to_string(), duplicate_paths.join(", "));
            } else {
                entries[index]
                    .custom_fields
                    .insert("is_duplicate".to_string(), "true".to_string());
                entries[index]
                    .custom_fields
                    .insert("original_path".to_string(), original_path.clone());
            }
        }
    }
}

fn promote_duplicate_fields(mut entry: proto::DecoratedEntry) -> proto::DecoratedEntry {
    for name in ["has_duplicates", "is_duplicate"] {
        if let Some(display) = entry.custom_fields.get(name).cloned() {
            entry.insert_field(name, value::boolean(display == "true"), display);
        }
    }
    if let Some(display) = entry.custom_fields.get("original_path").cloned() {
        entry.insert_field("original_path", value::path(&display), display);
    }
    entry
}

impl Default for DuplicateFileDetectorPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl ConfigurablePlugin for DuplicateFileDetectorPlugin {
    type Config = DuplicateConfig;

    fn config(&self) -> &Self::Config {
        self.base.config()
    }

    fn config_mut(&mut self) -> &mut Self::Config {
        self.base.config_mut()
    }
}

lla_plugin_sdk::export_plugin!(DuplicateFileDetectorPlugin);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batch_marks_the_original_and_every_duplicate() {
        let root = tempfile::tempdir().unwrap();
        let first = root.path().join("first.txt");
        let second = root.path().join("second.txt");
        std::fs::write(&first, "same").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        std::fs::write(&second, "same").unwrap();
        let mut entries = vec![
            proto::DecoratedEntry {
                path: first.to_string_lossy().into_owned(),
                ..Default::default()
            },
            proto::DecoratedEntry {
                path: second.to_string_lossy().into_owned(),
                ..Default::default()
            },
        ];
        mark_duplicate_groups(
            &mut entries,
            HashMap::from([("hash".to_string(), vec![0, 1])]),
        );

        assert_eq!(entries[0].custom_fields["has_duplicates"], "true");
        assert_eq!(entries[1].custom_fields["is_duplicate"], "true");
        assert_eq!(entries[1].custom_fields["original_path"], entries[0].path);
    }
}
