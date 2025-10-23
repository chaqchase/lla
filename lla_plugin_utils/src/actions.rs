use lla_plugin_interface::ActionInfo;
use std::collections::HashMap;

pub struct Action {
    pub handler: Box<dyn Fn(&[String]) -> Result<(), String> + Send + Sync>,
    pub help: ActionHelp,
}

pub struct ActionHelp {
    pub usage: String,
    pub description: String,
    pub examples: Vec<String>,
}

pub struct ActionRegistry {
    actions: HashMap<String, Action>,
}

impl ActionRegistry {
    pub fn new() -> Self {
        Self {
            actions: HashMap::new(),
        }
    }

    pub fn register<F>(&mut self, name: &str, help: ActionHelp, handler: F)
    where
        F: Fn(&[String]) -> Result<(), String> + Send + Sync + 'static,
    {
        self.actions.insert(
            name.to_string(),
            Action {
                handler: Box::new(handler),
                help,
            },
        );
    }

    pub fn handle(&self, action: &str, args: &[String]) -> Result<(), String> {
        match self.actions.get(action) {
            Some(action) => (action.handler)(args),
            None => Err(format!("Unknown action: {}", action)),
        }
    }

    pub fn get_help(&self) -> Vec<(&str, &ActionHelp)> {
        self.actions
            .iter()
            .map(|(name, action)| (name.as_str(), &action.help))
            .collect()
    }

    pub fn list_actions(&self) -> Vec<ActionInfo> {
        self.actions
            .iter()
            .map(|(name, action)| ActionInfo {
                name: name.clone(),
                usage: action.help.usage.clone(),
                description: action.help.description.clone(),
                examples: action.help.examples.clone(),
            })
            .collect()
    }
}

impl Default for ActionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[macro_export]
macro_rules! define_action {
    ($registry:expr, $name:expr, $usage:expr, $description:expr, $examples:expr, $handler:expr) => {
        $registry.register(
            $name,
            $crate::actions::ActionHelp {
                usage: $usage.to_string(),
                description: $description.to_string(),
                examples: $examples.iter().map(|s| s.to_string()).collect(),
            },
            $handler,
        );
    };
}
