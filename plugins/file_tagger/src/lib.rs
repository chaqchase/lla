use lazy_static::lazy_static;
use lla_plugin_sdk::{interface::proto, value, ActionArguments, DecoratedEntryExt, Plugin};
use lla_plugin_utils::{
    config::PluginConfig,
    decode_decorated_entry, map_decorated_entry, run_cli_action,
    ui::{
        components::{BoxComponent, BoxStyle, HelpFormatter, KeyValue, List, Spinner},
        TextBlock,
    },
    ActionRegistry, BasePlugin, ConfigurablePlugin,
};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs::File,
    io::{BufRead, BufReader, Write},
    path::PathBuf,
};

lazy_static! {
    static ref SPINNER: RwLock<Spinner> = RwLock::new(Spinner::new());
    static ref ACTION_REGISTRY: RwLock<ActionRegistry> = RwLock::new({
        let mut registry = ActionRegistry::new();

        lla_plugin_utils::define_action!(
            registry,
            "add-tag",
            "add-tag <file_path> <tag>",
            "Add a tag to a file",
            ["lla plugin run file_tagger add-tag -- \"/path/to/file\" \"mytag\""],
            |args| {
                if args.len() != 2 {
                    return Err("Usage: add-tag <file_path> <tag>".to_string());
                }
                let mut plugin = FileTaggerPlugin::new();
                plugin.add_tag(&args[0], &args[1]);
                println!(
                    "{}",
                    BoxComponent::new(
                        TextBlock::new(format!("Added tag '{}' to {}", args[1], args[0]))
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
            "remove-tag",
            "remove-tag <file_path> <tag>",
            "Remove a tag from a file",
            ["lla plugin run file_tagger remove-tag -- \"/path/to/file\" \"mytag\""],
            |args| {
                if args.len() != 2 {
                    return Err("Usage: remove-tag <file_path> <tag>".to_string());
                }
                let mut plugin = FileTaggerPlugin::new();
                plugin.remove_tag(&args[0], &args[1]);
                println!(
                    "{}",
                    BoxComponent::new(
                        TextBlock::new(format!("Removed tag '{}' from {}", args[1], args[0]))
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
            "list-tags",
            "list-tags <file_path>",
            "List all tags for a file",
            ["lla plugin run file_tagger list-tags -- \"/path/to/file\""],
            |args| {
                if args.len() != 1 {
                    return Err("Usage: list-tags <file_path>".to_string());
                }
                let plugin = FileTaggerPlugin::new();
                let tags = plugin.get_tags(&args[0]);
                let mut list = List::new().style(BoxStyle::Minimal).key_width(12);

                if tags.is_empty() {
                    list.add_item(
                        KeyValue::new("Info", format!("No tags found for {}", args[0]))
                            .key_color("bright_blue")
                            .value_color("bright_yellow")
                            .key_width(12)
                            .render(),
                    );
                } else {
                    list.add_item(
                        KeyValue::new("Tags", tags.join(", "))
                            .key_color("bright_green")
                            .value_color("bright_cyan")
                            .key_width(12)
                            .render(),
                    );
                }

                println!("\n{}", list.render());
                Ok(())
            }
        );

        lla_plugin_utils::define_action!(
            registry,
            "all-tags",
            "all-tags",
            "List all registered tags",
            ["lla plugin run file_tagger all-tags"],
            |_| {
                let plugin = FileTaggerPlugin::new();
                let tags = plugin.get_all_tags();
                let mut list = List::new().style(BoxStyle::Minimal).key_width(12);

                if tags.is_empty() {
                    list.add_item(
                        KeyValue::new("Info", "No tags found.")
                            .key_color("bright_blue")
                            .value_color("bright_yellow")
                            .key_width(12)
                            .render(),
                    );
                } else {
                    list.add_item(
                        KeyValue::new("All Tags", tags.join(", "))
                            .key_color("bright_green")
                            .value_color("bright_cyan")
                            .key_width(12)
                            .render(),
                    );
                }

                println!("\n{}", list.render());
                Ok(())
            }
        );

        lla_plugin_utils::define_action!(
            registry,
            "files-by-tag",
            "files-by-tag <tag-to-query>",
            "List all files tagged with the given tag",
            ["lla plugin run file_tagger files-by-tag -- \"tag-to-query\""],
            |args| {
                if args.len() != 1 {
                    return Err("Usage: files-by-tag <tag-to-query>".to_string());
                }
                let plugin = FileTaggerPlugin::new();
                let files = plugin.get_files_for_tag(&args[0]);
                let mut list = List::new().style(BoxStyle::Minimal).key_width(12);

                if files.is_empty() {
                    list.add_item(
                        KeyValue::new("Info", format!("No files found for tag [{}].", args[0]))
                            .key_color("bright_blue")
                            .value_color("bright_yellow")
                            .key_width(12)
                            .render(),
                    );
                } else {
                    list.add_item(
                        KeyValue::new(format!("All Files for [{}]", args[0]), files.join(", "))
                            .key_color("bright_green")
                            .value_color("bright_cyan")
                            .key_width(12)
                            .render(),
                    );
                }

                println!("\n{}", list.render());
                Ok(())
            }
        );
        lla_plugin_utils::define_action!(
            registry,
            "help",
            "help",
            "Show help information",
            ["lla plugin run file_tagger help"],
            |_| {
                let mut help = HelpFormatter::new("File Tagger Plugin".to_string());
                help.add_section("Description".to_string()).add_command(
                    "".to_string(),
                    "Add and manage tags for files.".to_string(),
                    vec![],
                );

                help.add_section("Actions".to_string())
                    .add_command(
                        "add-tag".to_string(),
                        "Add a tag to a file".to_string(),
                        vec![
                            "lla plugin run file_tagger add-tag -- \"/path/to/file\" \"mytag\""
                                .to_string(),
                        ],
                    )
                    .add_command(
                        "remove-tag".to_string(),
                        "Remove a tag from a file".to_string(),
                        vec![
                            "lla plugin run file_tagger remove-tag -- \"/path/to/file\" \"mytag\""
                                .to_string(),
                        ],
                    )
                    .add_command(
                        "list-tags".to_string(),
                        "List all tags for a file".to_string(),
                        vec![
                            "lla plugin run file_tagger list-tags -- \"/path/to/file\"".to_string()
                        ],
                    )
                    .add_command(
                        "all-tags".to_string(),
                        "List all registered tags across all files".to_string(),
                        vec!["lla plugin run file_tagger all-tags".to_string()],
                    )
                    .add_command(
                        "files-by-tag".to_string(),
                        "List all files tagged with the specific tag".to_string(),
                        vec![
                            "lla plugin run file_tagger files-by-tag -- \"tag-to-check\""
                                .to_string(),
                        ],
                    )
                    .add_command(
                        "help".to_string(),
                        "Show this help information".to_string(),
                        vec!["lla plugin run file_tagger help".to_string()],
                    );

                help.add_section("Formats".to_string())
                    .add_command(
                        "default".to_string(),
                        "Show tags in a compact format".to_string(),
                        vec![],
                    )
                    .add_command(
                        "long".to_string(),
                        "Show tags in a detailed format".to_string(),
                        vec![],
                    );

                println!(
                    "{}",
                    BoxComponent::new(help.render(&TaggerConfig::default().colors))
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
pub struct TaggerConfig {
    #[serde(default = "default_colors")]
    colors: HashMap<String, String>,
}

fn default_colors() -> HashMap<String, String> {
    let mut colors = HashMap::new();
    colors.insert("tag".to_string(), "bright_cyan".to_string());
    colors.insert("tag_label".to_string(), "bright_green".to_string());
    colors.insert("success".to_string(), "bright_green".to_string());
    colors.insert("info".to_string(), "bright_blue".to_string());
    colors.insert("name".to_string(), "bright_yellow".to_string());
    colors
}

impl Default for TaggerConfig {
    fn default() -> Self {
        Self {
            colors: default_colors(),
        }
    }
}

impl PluginConfig for TaggerConfig {}

pub struct FileTaggerPlugin {
    base: BasePlugin<TaggerConfig>,
    tag_file: PathBuf,
    tags: HashMap<String, Vec<String>>,
}

impl FileTaggerPlugin {
    pub fn new() -> Self {
        let plugin_name = env!("CARGO_PKG_NAME");
        let tag_file = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("lla")
            .join("file_tags.txt");
        let tags = Self::load_tags(&tag_file);
        let plugin = Self {
            base: BasePlugin::with_name(plugin_name),
            tag_file,
            tags,
        };
        if let Err(e) = plugin.base.save_config() {
            eprintln!("[FileTaggerPlugin] Failed to save config: {}", e);
        }
        plugin
    }

    fn load_tags(path: &PathBuf) -> HashMap<String, Vec<String>> {
        let mut tags: HashMap<String, Vec<String>> = HashMap::new();
        if let Ok(file) = File::open(path) {
            let reader = BufReader::new(file);
            for line in reader.lines().map_while(Result::ok) {
                let parts: Vec<&str> = line.split('|').collect();
                if parts.len() == 2 {
                    tags.entry(parts[0].to_string())
                        .or_default()
                        .push(parts[1].to_string());
                }
            }
        }
        tags
    }

    fn save_tags(&self) {
        if let Some(parent) = self.tag_file.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        if let Ok(mut file) = File::create(&self.tag_file) {
            for (file_path, tags) in &self.tags {
                for tag in tags {
                    writeln!(file, "{}|{}", file_path, tag).ok();
                }
            }
        }
    }

    fn add_tag(&mut self, file_path: &str, tag: &str) {
        self.tags
            .entry(file_path.to_string())
            .or_default()
            .push(tag.to_string());
        self.save_tags();
    }

    fn remove_tag(&mut self, file_path: &str, tag: &str) {
        if let Some(tags) = self.tags.get_mut(file_path) {
            tags.retain(|t| t != tag);
            if tags.is_empty() {
                self.tags.remove(file_path);
            }
        }
        self.save_tags();
    }

    fn get_tags(&self, file_path: &str) -> Vec<String> {
        self.tags.get(file_path).cloned().unwrap_or_default()
    }

    fn get_all_tags(&self) -> Vec<String> {
        let all_tags: std::collections::HashSet<_> =
            self.tags.values().flatten().cloned().collect();
        all_tags.into_iter().collect()
    }

    fn get_files_for_tag(&self, tag: &str) -> Vec<String> {
        let tag_string = tag.to_string();
        let all_files: Vec<String> = self
            .tags
            .iter()
            .filter(|(_, values)| values.contains(&tag_string))
            .map(|(key, _)| key.clone())
            .collect();
        all_files
    }

    fn format_tags(
        &self,
        entry: &lla_plugin_utils::DecoratedEntry,
        format: &str,
    ) -> Option<String> {
        let tags = entry.custom_fields.get("tags")?;
        if tags.is_empty() {
            return None;
        }

        let colors = &self.base.config().colors;
        let mut list = List::new().style(BoxStyle::Minimal).key_width(12);

        match format {
            "long" => {
                for tag in tags.split(", ") {
                    list.add_item(
                        KeyValue::new("Tag", tag)
                            .key_color(colors.get("tag_label").unwrap_or(&"white".to_string()))
                            .value_color(colors.get("tag").unwrap_or(&"white".to_string()))
                            .key_width(12)
                            .render(),
                    );
                }
                Some(format!("\n{}", list.render()))
            }
            "default" => {
                list.add_item(
                    KeyValue::new(
                        "Tags",
                        tags.split(", ")
                            .map(|t| format!("[{}]", t))
                            .collect::<Vec<_>>()
                            .join(" "),
                    )
                    .key_color(colors.get("tag_label").unwrap_or(&"white".to_string()))
                    .value_color(colors.get("tag").unwrap_or(&"white".to_string()))
                    .key_width(12)
                    .render(),
                );
                Some(format!("\n{}", list.render()))
            }
            _ => None,
        }
    }
}

impl Plugin for FileTaggerPlugin {
    fn decorate_entry(&mut self, entry: proto::DecoratedEntry) -> proto::DecoratedEntry {
        let mut entry = map_decorated_entry(entry, |mut entry| {
            let tags = self.get_tags(entry.path.to_str().unwrap_or(""));
            if !tags.is_empty() {
                entry.custom_fields.insert("tags".into(), tags.join(", "));
            }
            entry
        });
        if let Some(display) = entry.custom_fields.get("tags").cloned() {
            entry.insert_field("tags", value::string(&display), display);
        }
        entry
    }

    fn decorate_batch(
        &mut self,
        mut entries: Vec<proto::DecoratedEntry>,
        _format: &str,
    ) -> Vec<proto::DecoratedEntry> {
        for entry in &mut entries {
            if let Some(tags) = self.tags.get(&entry.path).filter(|tags| !tags.is_empty()) {
                let display = tags.join(", ");
                entry.insert_field("tags", value::string(&display), display);
            }
        }
        entries
    }

    fn format_field(&mut self, entry: proto::DecoratedEntry, format: String) -> Option<String> {
        decode_decorated_entry(entry)
            .ok()
            .and_then(|entry| self.format_tags(&entry, &format))
    }

    fn run_action(&mut self, action: String, arguments: ActionArguments) -> proto::ActionResponse {
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

impl Default for FileTaggerPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl ConfigurablePlugin for FileTaggerPlugin {
    type Config = TaggerConfig;

    fn config(&self) -> &Self::Config {
        self.base.config()
    }

    fn config_mut(&mut self) -> &mut Self::Config {
        self.base.config_mut()
    }
}

lla_plugin_sdk::export_plugin!(FileTaggerPlugin);
