use crate::error::Result;
use crate::plugin::PluginManager;
use lla_plugin_interface::proto::DecoratedEntry;

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
