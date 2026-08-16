use super::FileFormatter;
use crate::error::Result;
use crate::plugin::PluginManager;
use crate::utils::color::{colorize_file_name, colorize_file_name_with_icon};
use crate::utils::hyperlink;
use crate::utils::icons::format_with_icon;
use lla_plugin_interface::proto::DecoratedEntry;
use std::path::Path;
pub struct DefaultFormatter {
    pub show_icons: bool,
}

impl DefaultFormatter {
    pub fn new(show_icons: bool) -> Self {
        Self { show_icons }
    }
}
impl FileFormatter for DefaultFormatter {
    fn format_files(
        &self,
        files: &[DecoratedEntry],
        plugin_manager: &mut PluginManager,
        _depth: Option<usize>,
    ) -> Result<String> {
        plugin_manager.prepare_format_fields(files, "default");
        Ok(files
            .iter()
            .map(|file| {
                let path = Path::new(&file.path);
                let colored_name = colorize_file_name(path).to_string();
                let name_with_icon = colorize_file_name_with_icon(
                    path,
                    format_with_icon(path, colored_name, self.show_icons),
                )
                .to_string();
                let name_with_icon = hyperlink::link_path(path, name_with_icon);
                let plugin_fields = plugin_manager.format_fields(file, "default").join(" ");
                if plugin_fields.is_empty() {
                    name_with_icon
                } else {
                    format!("{} {}", name_with_icon, plugin_fields)
                }
            })
            .collect::<Vec<_>>()
            .join("\n"))
    }
}
