use lazy_static::lazy_static;
use lla_plugin_interface::{DecoratedEntry, Plugin, PluginRequest, PluginResponse};
use lla_plugin_utils::{
    config::PluginConfig,
    ui::{
        components::{BoxComponent, BoxStyle, HelpFormatter, KeyValue, List},
        TextBlock,
    },
    ActionRegistry, BasePlugin, ConfigurablePlugin, ProtobufHandler,
};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

lazy_static! {
    static ref ACTION_REGISTRY: RwLock<ActionRegistry> = RwLock::new({
        let mut registry = ActionRegistry::new();

        lla_plugin_utils::define_action!(
            registry,
            "add-category",
            "add-category <name> <color> <ext1,ext2,...> [description]",
            "Add a new category",
            vec!["lla plugin --name categorizer --action add-category --args Documents blue txt,doc,pdf \"Text documents\""],
            |args| {
                if args.len() < 3 {
                    return Err("Usage: add-category <name> <color> <ext1,ext2,...> [description]".to_string());
                }
                let mut rule = CategoryRule::default();
                rule.name = args[0].clone();
                rule.color = args[1].clone();
                rule.extensions = args[2].split(',').map(String::from).collect();
                if let Some(desc) = args.get(3) {
                    rule.description = desc.clone();
                }
                let mut plugin = FileCategoryPlugin::new();
                plugin.config_mut().rules.push(rule);
                plugin.base.save_config().map_err(|e| e.to_string())?;
                Ok(())
            }
        );

        lla_plugin_utils::define_action!(
            registry,
            "add-subcategory",
            "add-subcategory <category> <subcategory> <ext1,ext2,...>",
            "Add a subcategory to an existing category",
            vec!["lla plugin --name categorizer --action add-subcategory --args Documents Text txt,md"],
            |args| {
                if args.len() != 3 {
                    return Err(
                        "Usage: add-subcategory <category> <subcategory> <ext1,ext2,...>"
                            .to_string(),
                    );
                }

                let mut plugin = FileCategoryPlugin::new();
                let config = plugin.config_mut();

                if let Some(rule) = config.rules.iter_mut().find(|r| r.name == args[0]) {
                    rule.subcategories.insert(
                        args[1].to_string(),
                        args[2].split(',').map(String::from).collect(),
                    );
                    plugin.base.save_config().map_err(|e| e.to_string())?;
                    Ok(())
                } else {
                    Err(format!("Category '{}' not found", args[0]))
                }
            }
        );

        lla_plugin_utils::define_action!(
            registry,
            "list-categories",
            "list-categories",
            "List all categories and their details",
            vec!["lla plugin --name categorizer --action list-categories"],
            |_| {
                let plugin = FileCategoryPlugin::new();
                let mut list = List::new();
                for rule in &plugin.config().rules {
                    let mut details = Vec::new();
                    details.push(format!("Extensions: {}", rule.extensions.join(", ")));

                    if !rule.subcategories.is_empty() {
                        details.push("Subcategories:".to_string());
                        for (sub, exts) in &rule.subcategories {
                            details.push(format!("  {}: {}", sub, exts.join(", ")));
                        }
                    }

                    list.add_item(
                        KeyValue::new(&rule.name, &rule.description)
                            .key_color(&rule.color)
                            .key_width(15)
                            .render(),
                    );

                    for detail in details {
                        list.add_item("  ".to_string() + &detail);
                    }
                }

                println!(
                    "{}",
                    BoxComponent::new(list.render())
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
            vec!["lla plugin --name categorizer --action help"],
            |_| {
                let plugin = FileCategoryPlugin::new();
                let mut help = HelpFormatter::new("File Categorizer Plugin".to_string());
                help.add_section("Description".to_string()).add_command(
                    "".to_string(),
                    "Categorizes files based on their extensions and metadata".to_string(),
                    vec![],
                );

                help.add_section("Actions".to_string())
                    .add_command(
                        "add-category".to_string(),
                        "Add a new category".to_string(),
                        vec!["lla plugin --name categorizer --action add-category --args Documents blue txt,doc,pdf \"Text documents\"".to_string()],
                    )
                    .add_command(
                        "add-subcategory".to_string(),
                        "Add a subcategory to an existing category".to_string(),
                        vec!["lla plugin --name categorizer --action add-subcategory --args Documents Text txt,md".to_string()],
                    )
                    .add_command(
                        "list-categories".to_string(),
                        "List all categories and their details".to_string(),
                        vec!["lla plugin --name categorizer --action list-categories".to_string()],
                    )
                    .add_command(
                        "help".to_string(),
                        "Show this help information".to_string(),
                        vec!["lla plugin --name categorizer --action help".to_string()],
                    );

                println!(
                    "{}",
                    BoxComponent::new(help.render(&plugin.config().colors))
                        .style(BoxStyle::Minimal)
                        .padding(2)
                        .render()
                );
                Ok(())
            }
        );

        registry
    });
    static ref PLUGIN_STATE: RwLock<PluginState> = RwLock::new(PluginState::new());
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CategoryRule {
    name: String,
    color: String,
    extensions: Vec<String>,
    size_ranges: Option<Vec<(u64, u64)>>,
    subcategories: HashMap<String, Vec<String>>,
    description: String,
}

impl Default for CategoryRule {
    fn default() -> Self {
        Self {
            name: String::new(),
            color: "white".to_string(),
            extensions: Vec::new(),
            size_ranges: None,
            subcategories: HashMap::new(),
            description: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct CategoryStats {
    count: usize,
    total_size: u64,
    subcategory_counts: HashMap<String, usize>,
}

struct PluginState {
    stats: HashMap<String, CategoryStats>,
}

impl PluginState {
    fn new() -> Self {
        Self {
            stats: HashMap::new(),
        }
    }

    fn get_category_info(
        rules: &[CategoryRule],
        entry: &DecoratedEntry,
    ) -> Option<(String, String, Option<String>)> {
        let extension = entry.path.extension()?.to_str()?.to_lowercase();
        let size = entry.metadata.size;

        for rule in rules {
            if rule.extensions.iter().any(|ext| ext == &extension) {
                if let Some(ranges) = &rule.size_ranges {
                    if !ranges.iter().any(|(min, max)| size >= *min && size <= *max) {
                        continue;
                    }
                }

                let subcategory = rule
                    .subcategories
                    .iter()
                    .find(|(_, exts)| exts.contains(&extension))
                    .map(|(name, _)| name.clone());

                return Some((rule.name.clone(), rule.color.clone(), subcategory));
            }
        }
        None
    }

    fn update_stats(&mut self, entry: &DecoratedEntry, category: &str, subcategory: Option<&str>) {
        let stats = self.stats.entry(category.to_string()).or_default();
        stats.count += 1;
        stats.total_size += entry.metadata.size;
        if let Some(sub) = subcategory {
            *stats.subcategory_counts.entry(sub.to_string()).or_default() += 1;
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CategorizerConfig {
    #[serde(default = "default_colors")]
    colors: HashMap<String, String>,
    #[serde(default = "default_rules")]
    rules: Vec<CategoryRule>,
}

fn default_colors() -> HashMap<String, String> {
    let mut colors = HashMap::new();
    colors.insert("success".to_string(), "bright_green".to_string());
    colors.insert("info".to_string(), "bright_blue".to_string());
    colors.insert("name".to_string(), "bright_yellow".to_string());
    colors
}

fn default_rules() -> Vec<CategoryRule> {
    vec![
        CategoryRule {
            name: "Document".to_string(),
            color: "bright_blue".to_string(),
            extensions: vec!["txt", "md", "doc", "docx", "pdf", "rtf", "odt"]
                .into_iter()
                .map(String::from)
                .collect(),
            size_ranges: Some(vec![(0, 10_485_760)]),
            subcategories: {
                let mut map = HashMap::new();
                map.insert(
                    "Text".to_string(),
                    vec!["txt", "md"].into_iter().map(String::from).collect(),
                );
                map.insert(
                    "Office".to_string(),
                    vec!["doc", "docx", "xls", "xlsx", "ppt", "pptx"]
                        .into_iter()
                        .map(String::from)
                        .collect(),
                );
                map
            },
            description: "Text documents and office files".to_string(),
        },
        CategoryRule {
            name: "Code".to_string(),
            color: "bright_cyan".to_string(),
            extensions: vec![
                "rs", "py", "js", "ts", "java", "c", "cpp", "h", "hpp", "go", "rb", "php", "cs",
                "swift", "kt",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
            size_ranges: Some(vec![(0, 1_048_576)]),
            subcategories: {
                let mut map = HashMap::new();
                map.insert(
                    "Systems".to_string(),
                    vec!["rs", "c", "cpp", "h", "hpp"]
                        .into_iter()
                        .map(String::from)
                        .collect(),
                );
                map.insert(
                    "Web".to_string(),
                    vec!["js", "ts", "html", "css", "php"]
                        .into_iter()
                        .map(String::from)
                        .collect(),
                );
                map.insert(
                    "Scripts".to_string(),
                    vec!["py", "rb", "sh", "bash"]
                        .into_iter()
                        .map(String::from)
                        .collect(),
                );
                map
            },
            description: "Source code files".to_string(),
        },
    ]
}

impl Default for CategorizerConfig {
    fn default() -> Self {
        Self {
            colors: default_colors(),
            rules: default_rules(),
        }
    }
}

impl PluginConfig for CategorizerConfig {}

pub struct FileCategoryPlugin {
    base: BasePlugin<CategorizerConfig>,
}

impl FileCategoryPlugin {
    pub fn new() -> Self {
        let plugin_name = env!("CARGO_PKG_NAME");
        let plugin = Self {
            base: BasePlugin::with_name(plugin_name),
        };
        if let Err(e) = plugin.base.save_config() {
            eprintln!("[FileCategoryPlugin] Failed to save config: {}", e);
        }
        plugin
    }

    fn format_file_info(&self, entry: &DecoratedEntry, format: &str) -> Option<String> {
        match (
            entry.custom_fields.get("category"),
            entry.custom_fields.get("category_color"),
            entry.custom_fields.get("subcategory"),
        ) {
            (Some(category), Some(color), subcategory) => match format {
                "default" => Some(
                    TextBlock::new(format!("[{}]", category))
                        .color(color)
                        .build(),
                ),
                "long" => {
                    let base = TextBlock::new(format!("[{}]", category))
                        .color(color)
                        .build();
                    if let Some(sub) = subcategory {
                        Some(format!(
                            "{} ({})",
                            base,
                            TextBlock::new(sub).color("bright_black").build()
                        ))
                    } else {
                        Some(base)
                    }
                }
                _ => None,
            },
            _ => None,
        }
    }
}

impl Plugin for FileCategoryPlugin {
    fn handle_raw_request(&mut self, request: &[u8]) -> Vec<u8> {
        match self.decode_request(request) {
            Ok(request) => {
                let response = match request {
                    PluginRequest::GetName => {
                        PluginResponse::Name(env!("CARGO_PKG_NAME").to_string())
                    }
                    PluginRequest::GetVersion => {
                        PluginResponse::Version(env!("CARGO_PKG_VERSION").to_string())
                    }
                    PluginRequest::GetDescription => {
                        PluginResponse::Description(env!("CARGO_PKG_DESCRIPTION").to_string())
                    }
                    PluginRequest::GetSupportedFormats => PluginResponse::SupportedFormats(vec![
                        "default".to_string(),
                        "long".to_string(),
                    ]),
                    PluginRequest::Decorate(mut entry) => {
                        let mut state = PLUGIN_STATE.write();
                        if let Some((category, color, subcategory)) =
                            PluginState::get_category_info(&self.config().rules, &entry)
                        {
                            // Add category with rich type information
                            entry.custom_fields.insert("category".to_string(), category.clone());
                            entry.field_types.insert("category".to_string(), lla_plugin_interface::FieldType {
                                field_type: "badge".to_string(),
                                format: Some(color.clone()),
                                unit: None,
                            });

                            entry.custom_fields.insert("category_color".to_string(), color);
                            entry.field_types.insert("category_color".to_string(), lla_plugin_interface::FieldType {
                                field_type: "string".to_string(),
                                format: Some("color".to_string()),
                                unit: None,
                            });

                            if let Some(sub) = &subcategory {
                                entry.custom_fields.insert("subcategory".to_string(), sub.clone());
                                entry.field_types.insert("subcategory".to_string(), lla_plugin_interface::FieldType {
                                    field_type: "badge".to_string(),
                                    format: Some("gray".to_string()),
                                    unit: None,
                                });
                            }
                            state.update_stats(&entry, &category, subcategory.as_deref());
                        }
                        PluginResponse::Decorated(entry)
                    }
                    PluginRequest::BatchDecorate(mut entries, _format) => {
                        let mut state = PLUGIN_STATE.write();

                        for entry in &mut entries {
                            if let Some((category, color, subcategory)) =
                                PluginState::get_category_info(&self.config().rules, entry)
                            {
                                // Add category with rich type information
                                entry.custom_fields.insert("category".to_string(), category.clone());
                                entry.field_types.insert("category".to_string(), lla_plugin_interface::FieldType {
                                    field_type: "badge".to_string(),
                                    format: Some(color.clone()),
                                    unit: None,
                                });

                                entry.custom_fields.insert("category_color".to_string(), color);
                                entry.field_types.insert("category_color".to_string(), lla_plugin_interface::FieldType {
                                    field_type: "string".to_string(),
                                    format: Some("color".to_string()),
                                    unit: None,
                                });

                                if let Some(sub) = &subcategory {
                                    entry.custom_fields.insert("subcategory".to_string(), sub.clone());
                                    entry.field_types.insert("subcategory".to_string(), lla_plugin_interface::FieldType {
                                        field_type: "badge".to_string(),
                                        format: Some("gray".to_string()),
                                        unit: None,
                                    });
                                }
                                state.update_stats(entry, &category, subcategory.as_deref());
                            }
                        }

                        PluginResponse::BatchDecorated(entries)
                    }
                    PluginRequest::Config(_config_request) => {
                        // Plugin could adapt categories based on user theme preferences
                        PluginResponse::ConfigResult(Ok(()))
                    }
                    PluginRequest::FormatField(entry, format) => {
                        let field = self.format_file_info(&entry, &format);
                        PluginResponse::FormattedField(field)
                    }
                    PluginRequest::PerformAction(action, args) => {
                        let result = ACTION_REGISTRY.read().handle(&action, &args);
                        PluginResponse::ActionResult(result)
                    }
                };
                self.encode_response(response)
            }
            Err(e) => self.encode_error(&e),
        }
    }
}

impl Default for FileCategoryPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl ConfigurablePlugin for FileCategoryPlugin {
    type Config = CategorizerConfig;

    fn config(&self) -> &Self::Config {
        self.base.config()
    }

    fn config_mut(&mut self) -> &mut Self::Config {
        self.base.config_mut()
    }
}

impl ProtobufHandler for FileCategoryPlugin {}

lla_plugin_interface::declare_plugin!(FileCategoryPlugin);
