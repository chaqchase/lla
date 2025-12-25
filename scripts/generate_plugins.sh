#!/bin/bash

cd "$(dirname "$0")/.."

output_file="plugins.md"

cat > "$output_file" << EOL
# LLA Plugins

This document lists all available plugins for LLA and provides installation instructions.

## Installation

You can install all plugins at once using:

\`\`\`bash
lla install --git https://github.com/chaqchase/lla
\`\`\`

Or you can install individual plugins as described below.

## Available Plugins

EOL

for plugin_dir in plugins/*/; do
    if [ -f "${plugin_dir}Cargo.toml" ]; then
        name=$(grep '^name' "${plugin_dir}Cargo.toml" | sed 's/name = "\(.*\)"/\1/')
        version=$(grep '^version' "${plugin_dir}Cargo.toml" | sed 's/version = "\(.*\)"/\1/')
        description=$(grep '^description' "${plugin_dir}Cargo.toml" | sed 's/description = "\(.*\)"/\1/')
        
        if [ -z "$description" ]; then
            description="No description provided."
        fi
        
        readme_path="${plugin_dir}README.md"
        doc_link=""
        if [ -f "$readme_path" ]; then
            doc_link="[Documentation](${plugin_dir}README.md)"
        fi
        
        cat >> "$output_file" << EOL
### ${name}

**Description:** ${description}

**Version:** ${version}

**Documentation:** ${doc_link}

**Installation Options:**

1. Using LLA install command:
\`\`\`bash
lla install --dir path/to/lla/${plugin_dir}
\`\`\`

2. Manual installation:
\`\`\`bash
git clone https://github.com/chaqchase/lla
cd lla/${plugin_dir}
cargo build --release
\`\`\`

Then, copy the generated \`.so\`, \`.dll\`, or \`.dylib\` file from the \`target/release\` directory to your LLA plugins directory.

EOL
    fi
done

echo "Generated $output_file" 