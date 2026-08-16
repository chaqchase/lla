use lla_plugin_sdk::{interface::proto, Plugin};

#[derive(Default)]
struct Fixture;

impl Plugin for Fixture {
    fn decorate_batch(
        &mut self,
        mut entries: Vec<proto::DecoratedEntry>,
        _format: &str,
    ) -> Vec<proto::DecoratedEntry> {
        for entry in &mut entries {
            entry.custom_fields.insert("batch".into(), "native".into());
        }
        entries
    }
}

lla_plugin_sdk::export_plugin!(Fixture);
