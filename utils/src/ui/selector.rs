use super::components::LlaDialoguerTheme;
use dialoguer::{MultiSelect, Select};

pub fn select_single<T: ToString>(
    prompt: &str,
    items: &[T],
    default: Option<usize>,
) -> Result<usize, String> {
    let theme = LlaDialoguerTheme::default();
    let mut selector = Select::with_theme(&theme).with_prompt(prompt).items(items);

    if let Some(default_idx) = default {
        selector = selector.default(default_idx);
    }

    selector
        .interact()
        .map_err(|e| format!("Failed to show selector: {}", e))
}

pub fn select_multiple<T: ToString>(prompt: &str, items: &[T]) -> Result<Vec<usize>, String> {
    let theme = LlaDialoguerTheme::default();
    MultiSelect::with_theme(&theme)
        .with_prompt(prompt)
        .items(items)
        .interact()
        .map_err(|e| format!("Failed to show selector: {}", e))
}
