#[cfg(feature = "syntax")]
use lazy_static::lazy_static;
#[cfg(feature = "syntax")]
use syntect::{
    easy::HighlightLines,
    highlighting::{Style, ThemeSet},
    parsing::SyntaxSet,
    util::{as_24_bit_terminal_escaped, LinesWithEndings},
};

#[cfg(feature = "syntax")]
lazy_static! {
    static ref SYNTAX_SET: SyntaxSet = SyntaxSet::load_defaults_newlines();
    static ref THEME_SET: ThemeSet = ThemeSet::load_defaults();
}

pub struct CodeHighlighter;

impl CodeHighlighter {
    #[cfg(feature = "syntax")]
    pub fn highlight(code: &str, language: &str) -> String {
        let syntax = SYNTAX_SET
            .find_syntax_by_token(language)
            .unwrap_or_else(|| SYNTAX_SET.find_syntax_plain_text());
        let mut h = HighlightLines::new(syntax, &THEME_SET.themes["base16-ocean.dark"]);

        let mut highlighted = String::new();
        for line in LinesWithEndings::from(code) {
            let ranges: Vec<(Style, &str)> =
                h.highlight_line(line, &SYNTAX_SET).unwrap_or_default();
            let escaped = as_24_bit_terminal_escaped(&ranges[..], false);
            highlighted.push_str(&escaped);
        }
        highlighted
    }

    #[cfg(not(feature = "syntax"))]
    pub fn highlight(code: &str, _language: &str) -> String {
        code.to_string()
    }

    pub fn highlight_with_line_numbers(code: &str, language: &str, start_line: usize) -> String {
        let highlighted = Self::highlight(code, language);
        let mut result = String::new();
        for (i, line) in highlighted.lines().enumerate() {
            result.push_str(&format!("{:4} │ {}\n", i + start_line, line));
        }
        result
    }
}

#[cfg(feature = "syntax")]
pub fn get_available_themes() -> Vec<String> {
    THEME_SET.themes.keys().cloned().collect()
}

#[cfg(not(feature = "syntax"))]
pub fn get_available_themes() -> Vec<String> {
    vec![]
}
