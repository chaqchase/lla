use crate::commands::args::Args;
use crate::config::Config;
use ignore::WalkBuilder;
use std::path::Path;

pub fn build_walk_builder(root: &Path, args: &Args, config: &Config) -> WalkBuilder {
    let mut builder = WalkBuilder::new(root);
    let recursive_cfg = &config.listers.recursive;

    if recursive_cfg.hidden_follows_dotfiles {
        builder.hidden(args.no_dotfiles);
    } else {
        builder.hidden(false);
    }

    builder
        .git_ignore(args.respect_git_ignore)
        .git_global(args.use_git_global)
        .git_exclude(args.use_git_exclude)
        .parents(true);

    builder
}
