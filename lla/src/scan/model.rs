//! Turns scanned trees into categories of items you can act on.

use super::disk::Volume;
use super::rules::{artifact_info, Category, Mode, Rules, Safety};
use super::walk::Node;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Things untouched for this long count as probably unused.
pub const STALE_AFTER: i64 = 90 * 86_400;

pub struct Scan {
    pub roots: Vec<(PathBuf, Node)>,
    pub volume: Option<Volume>,
    pub elapsed: Duration,
    /// True when the user picked the folder instead of the default home scan.
    pub custom: bool,
}

impl Scan {
    /// The leftover group means something different when the user picked a folder.
    pub fn label(&self, category: Category) -> &'static str {
        match (category, self.custom) {
            (Category::System, true) => "Rest of the disk",
            _ => category.label(),
        }
    }

    pub fn about(&self, category: Category) -> &'static str {
        match (category, self.custom) {
            (Category::System, true) => "Everything on this disk outside the scanned folder.",
            _ => category.about(),
        }
    }

    pub fn scanned_bytes(&self) -> u64 {
        self.roots.iter().map(|(_, node)| node.size).sum()
    }

    pub fn files(&self) -> u64 {
        self.roots.iter().map(|(_, node)| node.files).sum()
    }

    pub fn unreadable(&self) -> u64 {
        self.roots.iter().map(|(_, node)| node.unreadable).sum()
    }

    /// Finds the node for an absolute path along with the root it belongs to.
    pub fn locate(&self, path: &Path) -> Option<&Node> {
        self.roots.iter().find_map(|(root, node)| {
            let rel = path.strip_prefix(root).ok()?;
            node.find(rel)
        })
    }

    pub fn remove(&mut self, path: &Path) -> Option<Node> {
        self.roots.iter_mut().find_map(|(root, node)| {
            let rel = path.strip_prefix(root).ok()?;
            if rel.as_os_str().is_empty() {
                return None;
            }
            node.remove(rel)
        })
    }

    pub fn insert(&mut self, folder: &Path, child: Node) -> bool {
        for (root, node) in &mut self.roots {
            if let Ok(rel) = folder.strip_prefix(root.as_path()) {
                return node.insert(rel, child);
            }
        }
        false
    }
}

#[derive(Clone, Debug)]
pub struct Item {
    pub path: PathBuf,
    /// Bytes that belong to this item and not to something nested in another category.
    pub size: u64,
    pub files: u64,
    pub newest: i64,
    pub dir: bool,
    pub safety: Safety,
    pub reason: &'static str,
    /// Lives in a trash already, so cleaning deletes instead of trashing.
    pub permanent: bool,
    pub protected: bool,
    /// Loose small files of a folder, shown as one row and not removable as a unit.
    pub loose: bool,
}

impl Item {
    pub fn stale(&self, now: i64) -> bool {
        self.newest > 0 && now - self.newest > STALE_AFTER
    }

    pub fn cleanable(&self, now: i64) -> bool {
        // Personal files are never suggested, however old they are.
        !self.loose
            && !self.protected
            && match self.safety {
                Safety::Safe => true,
                Safety::Review => self.stale(now),
                Safety::Keep => false,
            }
    }
}

pub struct Group {
    pub category: Category,
    pub size: u64,
    pub items: Vec<Item>,
    pub safe: u64,
    pub stale: u64,
}

pub struct Report {
    pub groups: Vec<Group>,
    pub now: i64,
}

impl Report {
    pub fn group(&self, category: Category) -> &Group {
        &self.groups[category.index()]
    }

    pub fn safe_total(&self) -> u64 {
        self.groups.iter().map(|group| group.safe).sum()
    }

    /// Non-empty groups, largest first, with the system remainder last.
    pub fn ordered(&self) -> Vec<Category> {
        let mut list: Vec<Category> = self
            .groups
            .iter()
            .filter(|group| group.size > 0 && group.category != Category::System)
            .map(|group| group.category)
            .collect();
        list.sort_by_key(|category| std::cmp::Reverse(self.group(*category).size));
        if self.group(Category::System).size > 0 {
            list.push(Category::System);
        }
        list
    }
}

#[derive(Clone, Copy)]
struct Context {
    category: Category,
    safety: Safety,
    reason: &'static str,
    permanent: bool,
    /// Children of this folder become items.
    container: bool,
    /// The item that currently owns the bytes, as (group, index).
    owner: Option<(usize, usize)>,
}

struct Builder<'a> {
    rules: &'a Rules,
    groups: Vec<Group>,
}

impl Builder<'_> {
    fn add_item(&mut self, ctx: &Context, path: &Path, node: &Node) -> (usize, usize) {
        let group = ctx.category.index();
        let items = &mut self.groups[group].items;
        items.push(Item {
            path: path.to_path_buf(),
            size: node.size,
            files: node.files,
            newest: node.newest,
            dir: node.dir,
            safety: ctx.safety,
            reason: ctx.reason,
            permanent: ctx.permanent,
            protected: self.rules.is_protected(path),
            loose: false,
        });
        (group, items.len() - 1)
    }

    fn shrink_owner(&mut self, owner: Option<(usize, usize)>, bytes: u64, files: u64) {
        if let Some((group, index)) = owner {
            let item = &mut self.groups[group].items[index];
            item.size = item.size.saturating_sub(bytes);
            item.files = item.files.saturating_sub(files);
        }
    }

    fn visit(&mut self, node: &Node, path: &mut PathBuf, parent: Context) {
        let rule = self.rules.exact(path).cloned();
        let artifact = artifact_info(node.tag);

        let mut ctx = parent;
        ctx.container = false;
        let starts_entry = if let Some(rule) = &rule {
            ctx.category = rule.category;
            ctx.safety = rule.safety;
            ctx.reason = rule.reason;
            ctx.permanent = rule.permanent;
            ctx.container = rule.mode == Mode::Container;
            true
        } else if let Some(artifact) = artifact {
            ctx.category = Category::Developer;
            ctx.safety = artifact.safety;
            ctx.reason = artifact.reason;
            ctx.permanent = false;
            true
        } else {
            parent.container
        };

        if starts_entry {
            // Bytes that start a new entry no longer belong to the enclosing item.
            self.shrink_owner(parent.owner, node.size, node.files);
            if ctx.container {
                ctx.owner = None;
                if node.small > 0 {
                    let group = ctx.category.index();
                    self.groups[group].items.push(Item {
                        path: path.clone(),
                        size: node.small,
                        files: node.small_count,
                        newest: node.newest,
                        dir: false,
                        safety: ctx.safety,
                        reason: ctx.reason,
                        permanent: ctx.permanent,
                        protected: true,
                        loose: true,
                    });
                }
            } else {
                ctx.owner = Some(self.add_item(&ctx, path, node));
            }
        }

        // Folders inside build artifacts never start anything new.
        if node.tag != 0 && rule.is_none() {
            return;
        }
        for child in &node.children {
            path.push(&*child.name);
            if child.dir || ctx.container {
                self.visit(child, path, ctx);
            }
            path.pop();
        }
    }
}

pub fn build(scan: &Scan, rules: &Rules, now: i64) -> Report {
    let mut builder = Builder {
        rules,
        groups: Category::ALL
            .iter()
            .map(|category| Group {
                category: *category,
                size: 0,
                items: Vec::new(),
                safe: 0,
                stale: 0,
            })
            .collect(),
    };

    for (root, node) in &scan.roots {
        // A folder picked by hand inherits whatever encloses it, and lists its children.
        let base = match rules.enclosing(root) {
            Some(rule) => Context {
                category: rule.category,
                safety: rule.safety,
                reason: rule.reason,
                permanent: rule.permanent,
                container: true,
                owner: None,
            },
            None => Context {
                category: Category::Other,
                safety: Safety::Review,
                reason: "in the scanned folder",
                permanent: false,
                container: true,
                owner: None,
            },
        };
        let mut path = root.clone();
        if rules.exact(root).is_some() || node.tag != 0 {
            let parent = Context {
                container: false,
                ..base
            };
            builder.visit(node, &mut path, parent);
        } else {
            // Treat the picked folder like a container of the inherited category.
            let rule_less = base;
            if node.small > 0 {
                builder.groups[rule_less.category.index()].items.push(Item {
                    path: root.clone(),
                    size: node.small,
                    files: node.small_count,
                    newest: node.newest,
                    dir: false,
                    safety: rule_less.safety,
                    reason: rule_less.reason,
                    permanent: rule_less.permanent,
                    protected: true,
                    loose: true,
                });
            }
            for child in &node.children {
                path.push(&*child.name);
                builder.visit(child, &mut path, rule_less);
                path.pop();
            }
        }
    }

    let mut groups = builder.groups;
    for group in &mut groups {
        group.items.retain(|item| item.size > 0);
        group
            .items
            .sort_unstable_by(|a, b| b.size.cmp(&a.size).then_with(|| a.path.cmp(&b.path)));
        group.size = group.items.iter().map(|item| item.size).sum();
        group.safe = group
            .items
            .iter()
            .filter(|item| item.safety == Safety::Safe && !item.loose && !item.protected)
            .map(|item| item.size)
            .sum();
        group.stale = group
            .items
            .iter()
            .filter(|item| item.stale(now) && !item.loose)
            .map(|item| item.size)
            .sum();
    }

    // Whatever the volume uses beyond what we could see.
    if let Some(volume) = &scan.volume {
        let seen: u64 = groups.iter().map(|group| group.size).sum();
        let rest = volume.used().saturating_sub(seen);
        groups[Category::System.index()].size = rest;
    }

    Report { groups, now }
}

/// Follows the largest child while it holds most of its parent, so a 6 GB folder
/// can say "mostly in Chrome/Default/Service Worker".
pub fn culprit(node: &Node) -> Option<(PathBuf, u64)> {
    let mut current = node;
    let mut rel = PathBuf::new();
    for _ in 0..8 {
        let Some(child) = current.children.first() else {
            break;
        };
        if current.size == 0 || child.size * 2 < current.size {
            break;
        }
        rel.push(&*child.name);
        current = child;
        if !child.dir {
            break;
        }
    }
    if rel.as_os_str().is_empty() || current.size * 5 < node.size {
        None
    } else {
        Some((rel, current.size))
    }
}

#[cfg(test)]
mod tests {
    use super::super::rules::Rule;
    use super::super::walk::{scan, Progress};
    use super::*;
    use std::fs;

    fn rule(path: PathBuf, category: Category, mode: Mode, safety: Safety) -> Rule {
        Rule {
            path,
            category,
            mode,
            safety,
            reason: "test",
            permanent: false,
        }
    }

    #[test]
    fn nested_artifacts_move_to_developer_and_leave_their_parent() {
        let home = tempfile::TempDir::new().unwrap();
        let home_path = home.path().canonicalize().unwrap();
        let docs = home_path.join("Documents");
        fs::create_dir_all(docs.join("site/node_modules/react")).unwrap();
        fs::create_dir_all(home_path.join("Downloads")).unwrap();
        fs::write(docs.join("site/package.json"), "{}").unwrap();
        fs::write(
            docs.join("site/node_modules/react/index.js"),
            vec![0u8; 3 << 20],
        )
        .unwrap();
        fs::write(docs.join("site/notes.txt"), vec![0u8; 2 << 20]).unwrap();
        fs::write(home_path.join("Downloads/a.iso"), vec![0u8; 2 << 20]).unwrap();

        let rules = Rules::new(
            Some(home_path.clone()),
            vec![
                rule(
                    home_path.clone(),
                    Category::Other,
                    Mode::Container,
                    Safety::Review,
                ),
                rule(
                    docs.clone(),
                    Category::Documents,
                    Mode::Container,
                    Safety::Keep,
                ),
                rule(
                    home_path.join("Downloads"),
                    Category::Downloads,
                    Mode::Container,
                    Safety::Review,
                ),
            ],
        );
        let node = scan(&home_path, &Progress::default());
        let scanned = Scan {
            roots: vec![(home_path.clone(), node)],
            volume: None,
            elapsed: Duration::ZERO,
            custom: false,
        };
        let report = build(&scanned, &rules, 0);

        let dev = report.group(Category::Developer);
        assert_eq!(dev.items.len(), 1);
        assert!(dev.items[0].path.ends_with("site/node_modules"));
        assert_eq!(dev.items[0].safety, Safety::Safe);

        let docs_group = report.group(Category::Documents);
        let site = docs_group
            .items
            .iter()
            .find(|item| item.path.ends_with("site"))
            .unwrap();
        // The site folder no longer counts the node_modules inside it.
        assert!(site.size < 3 << 20, "{}", site.size);
        assert!(site.size >= 2 << 20);

        let downloads = report.group(Category::Downloads);
        assert!(downloads.items[0].path.ends_with("a.iso"));

        // Everything except the folders' own blocks lands in exactly one group.
        let total: u64 = report.groups.iter().map(|g| g.size).sum();
        assert!(total <= scanned.scanned_bytes());
        assert!(scanned.scanned_bytes() - total < 1 << 20);
    }

    #[test]
    fn culprit_follows_the_dominant_child() {
        let root = tempfile::TempDir::new().unwrap();
        fs::create_dir_all(root.path().join("a/b/c")).unwrap();
        fs::write(root.path().join("a/b/c/big"), vec![0u8; 4 << 20]).unwrap();
        fs::write(root.path().join("a/small"), vec![0u8; 1 << 20]).unwrap();
        let node = scan(root.path(), &Progress::default());
        let a = node.find(Path::new("a")).unwrap();
        let (rel, size) = culprit(a).unwrap();
        assert_eq!(rel, Path::new("b/c/big"));
        assert!(size >= 4 << 20);
    }
}
