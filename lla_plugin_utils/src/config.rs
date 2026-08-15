use serde::{de::DeserializeOwned, Serialize};
use std::path::PathBuf;

pub trait PluginConfig: Default + Serialize + DeserializeOwned {
    fn validate(&self) -> Result<(), String> {
        Ok(())
    }
}

pub struct ConfigManager<T: PluginConfig> {
    config: T,
    config_path: PathBuf,
}

impl<T: PluginConfig> ConfigManager<T> {
    pub fn new(plugin_name: &str) -> Self {
        let config_root = std::env::var_os("LLA_PLUGIN_DATA_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                dirs::home_dir()
                    .unwrap_or_else(|| PathBuf::from("."))
                    .join(".config")
                    .join("lla")
                    .join("plugins")
            });
        let config_path = config_root.join(plugin_name).join("config.toml");

        if let Some(parent) = config_path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                eprintln!("[ConfigManager] Failed to create config directory: {}", e);
            }
        }

        let config = if config_path.exists() {
            Self::load_config(&config_path).unwrap_or_else(|e| {
                eprintln!(
                    "[ConfigManager] Failed to load config: {}, using default",
                    e
                );
                T::default()
            })
        } else {
            let config = T::default();
            match toml::to_string_pretty(&config) {
                Ok(content) => {
                    if let Err(e) = std::fs::write(&config_path, content) {
                        eprintln!("[ConfigManager] Failed to write initial config: {}", e);
                    }
                }
                Err(e) => eprintln!("[ConfigManager] Failed to serialize default config: {}", e),
            }
            config
        };

        Self {
            config,
            config_path,
        }
    }

    pub fn get(&self) -> &T {
        &self.config
    }

    pub fn get_mut(&mut self) -> &mut T {
        &mut self.config
    }

    pub fn save(&self) -> Result<(), String> {
        self.config.validate()?;
        if let Some(parent) = self.config_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create config directory: {}", e))?;
        }
        let content = toml::to_string_pretty(&self.config)
            .map_err(|e| format!("Failed to serialize config: {}", e))?;
        std::fs::write(&self.config_path, content)
            .map_err(|e| format!("Failed to write config file: {}", e))?;
        Ok(())
    }

    pub fn reload(&mut self) -> Result<(), String> {
        if let Ok(new_config) = Self::load_config(&self.config_path) {
            self.config = new_config;
            Ok(())
        } else {
            Err("Failed to reload configuration".to_string())
        }
    }

    fn load_config(path: &PathBuf) -> Result<T, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("Failed to read config file: {}", e))?;

        let config: T =
            toml::from_str(&content).map_err(|e| format!("Failed to parse config file: {}", e))?;

        config.validate()?;

        Ok(config)
    }
}

pub struct ConfigBuilder<T: Default> {
    config: T,
}

impl<T: Default> ConfigBuilder<T> {
    pub fn new() -> Self {
        Self {
            config: T::default(),
        }
    }

    pub fn build(self) -> T {
        self.config
    }
}

impl<T: Default> Default for ConfigBuilder<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[macro_export]
macro_rules! plugin_config {
    (
        $(#[$meta:meta])*
        pub struct $name:ident {
            $(
                $(#[$field_meta:meta])*
                pub $field:ident: $type:ty $(= $default:expr)?
            ),* $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
        pub struct $name {
            $(
                $(#[$field_meta])*
                pub $field: $type,
            )*
        }

        impl Default for $name {
            fn default() -> Self {
                Self {
                    $(
                        $field: plugin_config!(@default $($default)?),
                    )*
                }
            }
        }

        impl $crate::config::PluginConfig for $name {}
    };

    (@default) => {
        Default::default()
    };

    (@default $expr:expr) => {
        $expr
    };
}
