# lla Plugins

This is a list of all the plugins available for lla.

## Installation

You can install all prebuilt plugins at once using the default installer:

```bash
lla install
```

To force fetching the latest prebuilt archive (useful after a release):

```bash
# install pre-built plugins
lla install --prebuilt
# or install from a repo
lla install --git https://github.com/triyanox/lla
```

If you prefer to build from source, install them like this:

```bash
git clone https://github.com/triyanox/lla
cd lla/plugins/
cargo build --release
```

then copy the generated `.so`, `.dll`, or `.dylib` file from the `target/release` directory to your lla plugins directory.

## Available Plugins

- [categorizer](https://github.com/chaqchase/lla/tree/main/plugins/categorizer): Categorizes files based on their extensions and metadata
- [code_complexity](https://github.com/chaqchase/lla/tree/main/plugins/code_complexity): Analyzes code complexity using various metrics
- [code_snippet_extractor](https://github.com/chaqchase/lla/tree/main/plugins/code_snippet_extractor): A plugin for extracting and managing code snippets
- [dirs_meta](https://github.com/chaqchase/lla/tree/main/plugins/dirs_meta): Shows directories metadata
- [duplicate_file_detector](https://github.com/chaqchase/lla/tree/main/plugins/duplicate_file_detector): A plugin for the lla that detects duplicate files.
- [file_hash](https://github.com/chaqchase/lla/tree/main/plugins/file_hash): Displays the hash of each file
- [file_meta](https://github.com/chaqchase/lla/tree/main/plugins/file_meta): Displays the file metadata of each file
- [file_tagger](https://github.com/chaqchase/lla/tree/main/plugins/file_tagger): A plugin for tagging files and filtering by tags
- [git_status](https://github.com/chaqchase/lla/tree/main/plugins/git_status): Shows the git status of each file
- [keyword_search](https://github.com/chaqchase/lla/tree/main/plugins/keyword_search): Searches file contents for user-specified keywords
- [last_git_commit](https://github.com/chaqchase/lla/tree/main/plugins/last_git_commit): A plugin for the lla that provides the last git commit hash
- [sizeviz](https://github.com/chaqchase/lla/tree/main/plugins/sizeviz): File size visualizer plugin for lla
- [file_mover](https://github.com/chaqchase/lla/tree/main/plugins/file_mover): A plugin that provides an intuitive clipboard-based interface for moving files and directories.
- [file_copier](https://github.com/chaqchase/lla/tree/main/plugins/file_copier): A plugin that provides an intuitive clipboard-based interface for copying files and directories.
- [file_remover](https://github.com/chaqchase/lla/tree/main/plugins/file_remover): A plugin that provides an interactive interface for safely removing files and directories.
- [file_organizer](https://github.com/chaqchase/lla/tree/main/plugins/file_organizer): A plugin for organizing files using various strategies
