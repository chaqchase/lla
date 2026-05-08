use super::{TextBlock, TextStyle};
use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};
use std::cmp;
use std::time::Duration;

type HelpCommand = (String, String, Vec<String>);
type HelpSection = (String, Vec<HelpCommand>);

pub struct Spinner {
    progress_bar: ProgressBar,
}

impl Spinner {
    pub fn new() -> Self {
        let pb = ProgressBar::new_spinner();
        let style = ProgressStyle::default_spinner()
            .template("{spinner:.green} {msg}")
            .unwrap_or_else(|_| ProgressStyle::default_spinner())
            .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏");
        pb.set_style(style);
        pb.enable_steady_tick(Duration::from_millis(80));
        Self { progress_bar: pb }
    }

    pub fn set_status(&self, status: impl Into<String>) {
        self.progress_bar.set_message(status.into());
    }

    pub fn finish(&self) {
        self.progress_bar.finish_and_clear();
    }

    pub fn finish_with_message(&self, msg: impl Into<String>) {
        self.progress_bar.finish_with_message(msg.into());
    }
}

impl Default for Spinner {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Spinner {
    fn drop(&mut self) {
        self.finish();
    }
}

pub struct HelpFormatter {
    title: String,
    sections: Vec<HelpSection>,
}

impl HelpFormatter {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            sections: Vec::new(),
        }
    }

    pub fn add_section(&mut self, title: impl Into<String>) -> &mut Self {
        self.sections.push((title.into(), Vec::new()));
        self
    }

    pub fn add_command(
        &mut self,
        command: impl Into<String>,
        description: impl Into<String>,
        examples: Vec<String>,
    ) -> &mut Self {
        if let Some((_, commands)) = self.sections.last_mut() {
            commands.push((command.into(), description.into(), examples));
        }
        self
    }

    pub fn render(&self, colors: &std::collections::HashMap<String, String>) -> String {
        let mut output = String::new();
        let default_color = "white".to_string();
        let title_color = colors.get("success").unwrap_or(&default_color);
        let section_color = colors.get("info").unwrap_or(&default_color);
        let cmd_color = colors.get("name").unwrap_or(&default_color);

        output.push_str(
            &TextBlock::new(&self.title)
                .color(title_color)
                .style(TextStyle::Bold)
                .build(),
        );
        output.push_str("\n\n");

        for (section_title, commands) in &self.sections {
            output.push_str(&TextBlock::new(section_title).color(section_color).build());
            output.push_str("\n\n");

            for (command, description, examples) in commands {
                output.push_str("  ");
                output.push_str(
                    &TextBlock::new(command)
                        .color(cmd_color)
                        .style(TextStyle::Bold)
                        .build(),
                );
                output.push_str("\n    ");
                output.push_str(description);
                output.push('\n');

                if !examples.is_empty() {
                    output.push_str("\n    Examples:\n");
                    for example in examples {
                        output.push_str("      • ");
                        output.push_str(example);
                        output.push('\n');
                    }
                }
                output.push('\n');
            }
        }

        output
    }
}

pub struct KeyValue {
    key: String,
    value: String,
    key_color: Option<String>,
    value_color: Option<String>,
    key_width: Option<usize>,
}

impl KeyValue {
    pub fn new(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            value: value.into(),
            key_color: None,
            value_color: None,
            key_width: None,
        }
    }

    pub fn key_color(mut self, color: impl Into<String>) -> Self {
        self.key_color = Some(color.into());
        self
    }

    pub fn value_color(mut self, color: impl Into<String>) -> Self {
        self.value_color = Some(color.into());
        self
    }

    pub fn key_width(mut self, width: usize) -> Self {
        self.key_width = Some(width);
        self
    }

    pub fn render(&self) -> String {
        let key = if let Some(color) = &self.key_color {
            TextBlock::new(&self.key).color(color).build()
        } else {
            self.key.clone()
        };

        let value = if let Some(color) = &self.value_color {
            TextBlock::new(&self.value).color(color).build()
        } else {
            self.value.clone()
        };

        if let Some(width) = self.key_width {
            format!("{:width$} {}", key, value, width = width)
        } else {
            format!("{} {}", key, value)
        }
    }
}

pub struct List {
    items: Vec<String>,
}

impl List {
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }

    pub fn add_item(&mut self, item: impl Into<String>) -> &mut Self {
        self.items.push(item.into());
        self
    }

    pub fn style(self, _style: BoxStyle) -> Self {
        self
    }

    pub fn key_width(self, _width: usize) -> Self {
        self
    }

    pub fn render(&self) -> String {
        let mut output = String::new();
        output.push('┌');
        output.push('─');
        output.push('\n');

        for item in &self.items {
            output.push('│');
            output.push(' ');
            output.push_str(item);
            output.push('\n');
        }

        output.push('└');
        output.push('─');
        output.push('\n');
        output
    }
}

impl Default for List {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy)]
pub enum BoxStyle {
    Minimal,
    Rounded,
    Double,
    Heavy,
    Dashed,
}

impl BoxStyle {
    fn get_chars(&self) -> BoxChars {
        match self {
            BoxStyle::Minimal => BoxChars {
                top_left: '┌',
                top_right: '┐',
                bottom_left: '└',
                bottom_right: '┘',
                horizontal: '─',
                vertical: '│',
                left_t: '├',
                right_t: '┤',
                top_t: '┬',
                bottom_t: '┴',
                cross: '┼',
            },
            BoxStyle::Rounded => BoxChars {
                top_left: '╭',
                top_right: '╮',
                bottom_left: '╰',
                bottom_right: '╯',
                horizontal: '─',
                vertical: '│',
                left_t: '├',
                right_t: '┤',
                top_t: '┬',
                bottom_t: '┴',
                cross: '┼',
            },
            BoxStyle::Double => BoxChars {
                top_left: '╔',
                top_right: '╗',
                bottom_left: '╚',
                bottom_right: '╝',
                horizontal: '═',
                vertical: '║',
                left_t: '╠',
                right_t: '╣',
                top_t: '╦',
                bottom_t: '╩',
                cross: '╬',
            },
            BoxStyle::Heavy => BoxChars {
                top_left: '┏',
                top_right: '┓',
                bottom_left: '┗',
                bottom_right: '┛',
                horizontal: '━',
                vertical: '┃',
                left_t: '┣',
                right_t: '┫',
                top_t: '┳',
                bottom_t: '┻',
                cross: '╋',
            },
            BoxStyle::Dashed => BoxChars {
                top_left: '┌',
                top_right: '┐',
                bottom_left: '└',
                bottom_right: '┘',
                horizontal: '┄',
                vertical: '┆',
                left_t: '├',
                right_t: '┤',
                top_t: '┬',
                bottom_t: '┴',
                cross: '┼',
            },
        }
    }
}

#[allow(dead_code)]
struct BoxChars {
    top_left: char,
    top_right: char,
    bottom_left: char,
    bottom_right: char,
    horizontal: char,
    vertical: char,
    left_t: char,
    right_t: char,
    top_t: char,
    bottom_t: char,
    cross: char,
}

pub struct BoxComponent {
    content: String,
    style: BoxStyle,
    width: Option<usize>,
    padding: usize,
    title: Option<String>,
}

impl BoxComponent {
    pub fn new(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            style: BoxStyle::Minimal,
            width: None,
            padding: 0,
            title: None,
        }
    }

    pub fn style(mut self, style: BoxStyle) -> Self {
        self.style = style;
        self
    }

    pub fn width(mut self, width: usize) -> Self {
        self.width = Some(width);
        self
    }

    pub fn padding(mut self, padding: usize) -> Self {
        self.padding = padding;
        self
    }

    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn render(&self) -> String {
        let chars = self.style.get_chars();
        let mut output = String::new();
        output.push('\n');
        let lines: Vec<&str> = self.content.lines().collect();

        // 2 for left+right border chars
        let term_width = console::Term::stdout().size().1 as usize;
        let max_total_width = term_width.saturating_sub(2);

        let content_width = lines
            .iter()
            .map(|line| console::measure_text_width(line))
            .max()
            .unwrap_or(0);
        let title_width = self
            .title
            .as_ref()
            .map(|t| console::measure_text_width(t))
            .unwrap_or(0);
        let inner_width = cmp::max(content_width, title_width) + self.padding * 2;
        let desired_width = self.width.unwrap_or(inner_width);
        let total_width = cmp::min(desired_width, max_total_width);
        let max_content_visible = total_width.saturating_sub(self.padding * 2);

        output.push(chars.top_left);
        if let Some(title) = &self.title {
            let title_vis = console::measure_text_width(title);
            let available_for_title = total_width.saturating_sub(4);
            if title_vis <= available_for_title {
                output.push(chars.horizontal);
                output.push(' ');
                output.push_str(title);
                output.push(' ');
                let remaining = total_width.saturating_sub(title_vis + 4);
                output.push_str(&chars.horizontal.to_string().repeat(remaining));
            } else {
                let truncated = console::truncate_str(title, available_for_title, "…");
                output.push(chars.horizontal);
                output.push(' ');
                output.push_str(&truncated);
                output.push(' ');
                let trunc_vis = console::measure_text_width(&truncated);
                let remaining = total_width.saturating_sub(trunc_vis + 4);
                output.push_str(&chars.horizontal.to_string().repeat(remaining));
            }
        } else {
            output.push_str(&chars.horizontal.to_string().repeat(total_width));
        }
        output.push(chars.top_right);
        output.push('\n');

        for _ in 0..self.padding {
            output.push(chars.vertical);
            output.push_str(&" ".repeat(total_width));
            output.push(chars.vertical);
            output.push('\n');
        }

        for line in lines {
            let line_width = console::measure_text_width(line);
            output.push(chars.vertical);
            output.push_str(&" ".repeat(self.padding));
            if line_width <= max_content_visible {
                output.push_str(line);
                let padding = total_width.saturating_sub(line_width + self.padding);
                output.push_str(&" ".repeat(padding));
            } else {
                let truncated =
                    console::truncate_str(line, max_content_visible.saturating_sub(1), "…");
                let trunc_vis = console::measure_text_width(&truncated);
                output.push_str(&truncated);
                let padding = total_width.saturating_sub(trunc_vis + self.padding);
                output.push_str(&" ".repeat(padding));
            }
            output.push(chars.vertical);
            output.push('\n');
        }

        for _ in 0..self.padding {
            output.push(chars.vertical);
            output.push_str(&" ".repeat(total_width));
            output.push(chars.vertical);
            output.push('\n');
        }

        output.push(chars.bottom_left);
        output.push_str(&chars.horizontal.to_string().repeat(total_width));
        output.push(chars.bottom_right);
        output.push('\n');

        output
    }
}

pub struct LlaDialoguerTheme {
    colors: std::collections::HashMap<String, String>,
    symbols: std::collections::HashMap<String, String>,
    padding: usize,
}

impl LlaDialoguerTheme {
    pub fn new(colors: std::collections::HashMap<String, String>) -> Self {
        Self {
            colors,
            symbols: Self::default_symbols(),
            padding: 1,
        }
    }

    fn default_symbols() -> std::collections::HashMap<String, String> {
        let mut symbols = std::collections::HashMap::new();
        symbols.insert("error".to_string(), "✘".to_string());
        symbols.insert("success".to_string(), "✔".to_string());
        symbols.insert("pointer".to_string(), "›".to_string());
        symbols.insert("unchecked".to_string(), "○".to_string());
        symbols.insert("checked".to_string(), "●".to_string());
        symbols.insert("separator".to_string(), "•".to_string());
        symbols.insert("prompt".to_string(), "⟩".to_string());
        symbols.insert("bullet".to_string(), " ".to_string());
        symbols.insert("warning".to_string(), "⚠".to_string());
        symbols.insert("info".to_string(), "ℹ".to_string());
        symbols.insert("gradient_sep".to_string(), "· · ·".to_string());
        symbols
    }

    fn default_theme() -> Self {
        let mut colors = std::collections::HashMap::new();
        colors.insert("success".to_string(), "bright_green".to_string());
        colors.insert("info".to_string(), "cyan".to_string());
        colors.insert("error".to_string(), "red".to_string());
        colors.insert("path".to_string(), "yellow".to_string());
        colors.insert("prompt".to_string(), "bright_magenta".to_string());
        colors.insert("highlight".to_string(), "bright_white".to_string());
        colors.insert("inactive".to_string(), "bright_black".to_string());
        colors.insert("separator".to_string(), "bright_black".to_string());
        colors.insert("warning".to_string(), "yellow".to_string());
        colors.insert("accent".to_string(), "bright_blue".to_string());
        colors.insert("gradient1".to_string(), "bright_magenta".to_string());
        colors.insert("gradient2".to_string(), "magenta".to_string());
        colors.insert("gradient3".to_string(), "bright_black".to_string());
        Self::new(colors)
    }

    pub fn with_symbols(mut self, symbols: std::collections::HashMap<String, String>) -> Self {
        self.symbols.extend(symbols);
        self
    }

    pub fn with_padding(mut self, padding: usize) -> Self {
        self.padding = padding;
        self
    }

    fn get_color(&self, key: &str) -> &str {
        self.colors.get(key).map(|s| s.as_str()).unwrap_or("white")
    }

    fn get_symbol(&self, key: &str) -> &str {
        self.symbols.get(key).map(|s| s.as_str()).unwrap_or("")
    }

    fn format_gradient_separator(&self) -> String {
        format!(
            "{} {} {}",
            "·".color(self.get_color("gradient1")),
            "·".color(self.get_color("gradient2")),
            "·".color(self.get_color("gradient3"))
        )
    }

    fn format_select_prefix(&self, active: bool) -> String {
        if active {
            format!(
                " {} ",
                self.get_symbol("pointer")
                    .color(self.get_color("accent"))
                    .bold(),
            )
        } else {
            "   ".to_string()
        }
    }
}

impl Default for LlaDialoguerTheme {
    fn default() -> Self {
        Self::default_theme()
    }
}

impl dialoguer::theme::Theme for LlaDialoguerTheme {
    fn format_prompt(&self, f: &mut dyn std::fmt::Write, prompt: &str) -> std::fmt::Result {
        write!(
            f,
            "{} {} {} ",
            self.get_symbol("prompt").color(self.get_color("accent")),
            prompt.color(self.get_color("prompt")).bold(),
            self.format_gradient_separator()
        )
    }

    fn format_error(&self, f: &mut dyn std::fmt::Write, err: &str) -> std::fmt::Result {
        write!(
            f,
            "{} {} {}",
            self.get_symbol("error").color(self.get_color("error")),
            self.format_gradient_separator(),
            err.color(self.get_color("error")).bold()
        )
    }

    fn format_confirm_prompt(
        &self,
        f: &mut dyn std::fmt::Write,
        prompt: &str,
        default: Option<bool>,
    ) -> std::fmt::Result {
        let options = match default {
            Some(true) => format!(
                "[{}{}]",
                "Y".color(self.get_color("accent")).bold(),
                "/n".color(self.get_color("inactive"))
            ),
            Some(false) => format!(
                "[{}{}]",
                "y".color(self.get_color("inactive")),
                "/N".color(self.get_color("accent")).bold()
            ),
            None => format!("[{}]", "y/n".color(self.get_color("accent")).bold()),
        };
        write!(
            f,
            "{} {} {} ",
            self.get_symbol("prompt").color(self.get_color("accent")),
            prompt.color(self.get_color("prompt")).bold(),
            options
        )
    }

    fn format_select_prompt_item(
        &self,
        f: &mut dyn std::fmt::Write,
        text: &str,
        active: bool,
    ) -> std::fmt::Result {
        let padding = " ".repeat(self.padding);
        if active {
            write!(
                f,
                "{}{}{}",
                padding,
                self.format_select_prefix(active),
                text.color(self.get_color("highlight")).bold()
            )
        } else {
            write!(
                f,
                "{}{}{}",
                padding,
                self.format_select_prefix(active),
                text.color(self.get_color("inactive"))
            )
        }
    }

    fn format_multi_select_prompt_item(
        &self,
        f: &mut dyn std::fmt::Write,
        text: &str,
        checked: bool,
        active: bool,
    ) -> std::fmt::Result {
        let padding = " ".repeat(self.padding);
        let check_symbol = if checked {
            self.get_symbol("checked")
                .color(self.get_color("success"))
                .bold()
                .to_string()
        } else {
            self.get_symbol("unchecked")
                .color(self.get_color("inactive"))
                .to_string()
        };

        let text_style = match (active, checked) {
            (true, _) => text.color(self.get_color("highlight")).bold(),
            (false, true) => text.color(self.get_color("info")),
            (false, false) => text.color(self.get_color("inactive")),
        };

        write!(
            f,
            "{}{}{}  {}",
            padding,
            self.format_select_prefix(active),
            check_symbol,
            text_style
        )
    }
}
