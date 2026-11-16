use super::render_entry_name;
use super::FileFormatter;
use crate::error::Result;
use crate::plugin::PluginManager;
use lla_plugin_interface::proto::DecoratedEntry;
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
                let name_with_icon = render_entry_name(file, self.show_icons);
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
