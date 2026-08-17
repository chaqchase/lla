use lla_plugin_sdk::{interface::proto, Plugin};

#[derive(Default)]
struct Fixture;

impl Plugin for Fixture {
    fn decorate_entry(&mut self, mut entry: proto::DecoratedEntry) -> proto::DecoratedEntry {
        entry.custom_fields.insert("fixture".into(), "yes".into());
        entry
    }
}

lla_plugin_sdk::export_plugin!(Fixture);
