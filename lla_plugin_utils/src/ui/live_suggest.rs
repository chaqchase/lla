use super::components::LlaDialoguerTheme;
use console::{style, Key, Term};
use dialoguer::theme::Theme;
use std::thread;
use std::time::{Duration, Instant};

/// Interactive suggestions input loop with arrow selection and Enter to submit.
/// - `prompt`: Text shown above the input.
/// - `initial`: Optional initial text (e.g., clipboard selection).
/// - `fetch`: Callback to fetch suggestions based on current input. Called with throttling.
///
/// Returns the chosen query (either the typed input or a selected suggestion).
pub fn interactive_suggest<F>(
    prompt: &str,
    initial: Option<&str>,
    mut fetch: F,
) -> Result<String, String>
where
    F: FnMut(&str) -> Result<Vec<String>, String>,
{
    let term = Term::stdout();
    let theme = LlaDialoguerTheme::default();

    let mut input = initial.unwrap_or("").to_string();
    let mut suggestions: Vec<String> = Vec::new();
    let mut selected: usize = 0; // 0 = use current input
    let mut last_lines: usize = 0;
    let mut last_fetch_at = Instant::now() - Duration::from_millis(500);
    let mut last_render_at;
    let mut last_sent_query = String::new();
    let mut fetching = false;
    let spinner_frames: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
    let mut spinner_idx: usize = 0;
    let mut needs_render = false;

    const MAX_SUGGESTIONS: usize = 8; // Limit suggestions to prevent excessive layout shift
    const RENDER_DEBOUNCE_MS: u64 = 50; // Debounce rendering to prevent flickering
    const FETCH_THROTTLE_MS: u64 = 200; // Throttle API calls

    // Helper to (re)render the UI
    let render = |prev_lines: usize,
                  input: &str,
                  suggestions: &Vec<String>,
                  selected: usize,
                  fetching_now: bool,
                  spin_idx: usize|
     -> Result<usize, String> {
        // Build the display content
        let mut lines = Vec::new();

        // Header
        let mut header = String::new();
        Theme::format_prompt(&theme, &mut header, prompt)
            .map_err(|e| format!("Failed to format header: {}", e))?;

        // Input line with cursor indicator
        let input_display = if input.is_empty() {
            style("(type to search)").dim().to_string()
        } else {
            input.to_string()
        };

        lines.push(format!("{}{}", header, style(&input_display).cyan().bold()));
        lines.push(String::new());

        // Status line
        let status = if fetching_now {
            format!(
                "{} {}",
                style(spinner_frames[spin_idx]).cyan(),
                style("Fetching suggestions...").dim()
            )
        } else if input.trim().is_empty() {
            format!("{}", style("Start typing to see suggestions").dim())
        } else if suggestions.is_empty() {
            format!("{}", style("No suggestions found").yellow())
        } else {
            format!(
                "{} {} • Use ↑↓ to navigate, Enter to select",
                style("✓").green(),
                style(format!(
                    "{} suggestion{}",
                    suggestions.len().min(MAX_SUGGESTIONS),
                    if suggestions.len() == 1 { "" } else { "s" }
                ))
                .dim()
            )
        };
        lines.push(status);
        lines.push(style("─".repeat(60)).dim().to_string()); // Separator line
        lines.push(String::new());

        // Current input option
        let input_line = if selected == 0 {
            format!(
                "{} {} {}",
                style("❯").cyan().bold(),
                style("🔎").cyan(),
                style(if input.is_empty() {
                    "(empty query)"
                } else {
                    input
                })
                .cyan()
                .bold()
            )
        } else {
            format!(
                "  {} {}",
                style("🔎").dim(),
                style(if input.is_empty() {
                    "(empty query)"
                } else {
                    input
                })
                .dim()
            )
        };
        lines.push(input_line);

        // Suggestions (limit to prevent layout shift)
        let display_suggestions: Vec<_> = suggestions.iter().take(MAX_SUGGESTIONS).collect();

        for (i, s) in display_suggestions.iter().enumerate() {
            let is_selected = selected == i + 1;
            let suggestion_line = if is_selected {
                format!(
                    "{} {} {}",
                    style("❯").cyan().bold(),
                    style("💡").cyan(),
                    style(s).cyan().bold()
                )
            } else {
                format!("  {} {}", style("💡").dim(), style(s).dim())
            };
            lines.push(suggestion_line);
        }

        // Add empty lines to maintain consistent height (reduce layout shift)
        let min_lines = 12; // Minimum number of content lines to show
        while lines.len() < min_lines {
            lines.push(String::new());
        }

        // Add extra empty lines at the end to ensure clean clearing
        lines.push(String::new());
        lines.push(String::new());

        // Clear previous render first to prevent character leaks
        if prev_lines > 0 {
            term.clear_last_lines(prev_lines)
                .map_err(|e| format!("Failed to clear terminal: {}", e))?;
        }

        // Write new content directly without box border
        let total_lines = lines.len();
        for line in lines {
            term.write_line(&line)
                .map_err(|e| format!("Failed to write to terminal: {}", e))?;
        }

        Ok(total_lines)
    };

    // Initial render
    last_lines = render(
        last_lines,
        &input,
        &suggestions,
        selected,
        fetching,
        spinner_idx,
    )?;
    last_render_at = Instant::now();

    loop {
        let now = Instant::now();

        // Throttled suggestions fetching when input changes
        if !input.trim().is_empty()
            && (now.duration_since(last_fetch_at) >= Duration::from_millis(FETCH_THROTTLE_MS))
            && input != last_sent_query
        {
            fetching = true;
            spinner_idx = (spinner_idx + 1) % spinner_frames.len();

            // Show fetching state
            last_lines = render(
                last_lines,
                &input,
                &suggestions,
                selected,
                fetching,
                spinner_idx,
            )?;

            match fetch(&input) {
                Ok(list) => suggestions = list,
                Err(_) => suggestions.clear(),
            }
            last_fetch_at = now;
            last_sent_query = input.clone();
            // Clamp selection if needed
            let max_selection = suggestions.len().min(MAX_SUGGESTIONS);
            if selected > max_selection {
                selected = max_selection;
            }
            fetching = false;

            // Show results
            last_lines = render(
                last_lines,
                &input,
                &suggestions,
                selected,
                fetching,
                spinner_idx,
            )?;
            last_render_at = Instant::now();
        }
        // Debounced rendering for text input - only render if enough time has passed
        else if needs_render {
            let time_since_render = now.duration_since(last_render_at);
            if time_since_render >= Duration::from_millis(RENDER_DEBOUNCE_MS) {
                last_lines = render(
                    last_lines,
                    &input,
                    &suggestions,
                    selected,
                    fetching,
                    spinner_idx,
                )?;
                last_render_at = Instant::now();
                needs_render = false;
            } else {
                // Sleep for remaining debounce time
                thread::sleep(Duration::from_millis(RENDER_DEBOUNCE_MS) - time_since_render);
                last_lines = render(
                    last_lines,
                    &input,
                    &suggestions,
                    selected,
                    fetching,
                    spinner_idx,
                )?;
                last_render_at = Instant::now();
                needs_render = false;
            }
        }

        // Update spinner periodically when fetching
        if fetching && !needs_render {
            let time_since_render = Instant::now().duration_since(last_render_at);
            if time_since_render >= Duration::from_millis(100) {
                spinner_idx = (spinner_idx + 1) % spinner_frames.len();
                last_lines = render(
                    last_lines,
                    &input,
                    &suggestions,
                    selected,
                    fetching,
                    spinner_idx,
                )?;
                last_render_at = Instant::now();
            }
        }

        // Read a key (blocking)
        let key = term
            .read_key()
            .map_err(|e| format!("Failed to read key: {}", e))?;

        match key {
            Key::Char('\n') | Key::Enter => {
                // Return selected item (0 = input)
                if last_lines > 0 {
                    term.clear_last_lines(last_lines)
                        .map_err(|e| format!("Failed to clear terminal: {}", e))?;
                }
                if selected == 0 {
                    return Ok(input);
                } else {
                    return Ok(suggestions.get(selected - 1).cloned().unwrap_or_default());
                }
            }
            Key::Escape | Key::CtrlC => {
                if last_lines > 0 {
                    term.clear_last_lines(last_lines)
                        .map_err(|e| format!("Failed to clear terminal: {}", e))?;
                }
                return Err("Cancelled".to_string());
            }
            Key::Backspace => {
                input.pop();
                selected = 0;
                needs_render = true;
            }
            Key::Char(c) => {
                // Basic text input
                if !c.is_control() {
                    input.push(c);
                    selected = 0;
                    needs_render = true;
                }
            }
            Key::ArrowDown => {
                let total = 1 + suggestions.len().min(MAX_SUGGESTIONS);
                if total > 0 {
                    selected = (selected + 1) % total;
                    // Immediate render for navigation
                    last_lines = render(
                        last_lines,
                        &input,
                        &suggestions,
                        selected,
                        fetching,
                        spinner_idx,
                    )?;
                    last_render_at = Instant::now();
                    needs_render = false;
                }
            }
            Key::ArrowUp => {
                let total = 1 + suggestions.len().min(MAX_SUGGESTIONS);
                if total > 0 {
                    if selected == 0 {
                        selected = total - 1;
                    } else {
                        selected -= 1;
                    }
                    // Immediate render for navigation
                    last_lines = render(
                        last_lines,
                        &input,
                        &suggestions,
                        selected,
                        fetching,
                        spinner_idx,
                    )?;
                    last_render_at = Instant::now();
                    needs_render = false;
                }
            }
            // Ignore other keys for now
            _ => {}
        }
    }
}
