use crate::error::Result;
use crate::plugin::PluginManager;
use crate::utils::color::{colorize_file_name_as, FileKind};
use crate::utils::hyperlink;
use crate::utils::icons::format_with_icon_as;
use lla_plugin_interface::proto::DecoratedEntry;

/// Renders an entry's colored name, optional icon and hyperlink from the
/// metadata collected while listing, so no formatter re-stats the file.
pub(crate) fn render_entry_name(entry: &DecoratedEntry, show_icons: bool) -> String {
    let path = std::path::Path::new(&entry.path);
    let kind = FileKind::of_entry(path, entry.metadata.as_ref());
    let name = colorize_file_name_as(path, kind).to_string();
    let name = format_with_icon_as(path, kind, name, show_icons);
    hyperlink::link_listed(path, name)
}

pub trait FileFormatter {
    fn format_files(
        &self,
        files: &[DecoratedEntry],
        plugin_manager: &mut PluginManager,
        depth: Option<usize>,
    ) -> Result<String>;
}

pub mod column_config;
pub mod csv;
mod default;
mod fuzzy;
mod git;
mod grid;
pub mod json;
mod long;
mod recursive;
pub mod serializable;
mod sizemap;
mod table;
mod timeline;
mod tree;

pub use default::DefaultFormatter;
pub use fuzzy::FuzzyFormatter;
pub use git::GitFormatter;
pub use grid::GridFormatter;
pub use long::LongFormatter;
pub use recursive::RecursiveFormatter;
pub use sizemap::SizeMapFormatter;
pub use table::TableFormatter;
pub use timeline::TimelineFormatter;
pub use tree::TreeFormatter;
